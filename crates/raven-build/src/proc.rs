//! Reusable process-execution layer.
//!
//! Design goals (see `docs/architecture.md`):
//!
//! * **No deadlocks** — stdout and stderr are drained concurrently on
//!   separate threads while the parent waits on the child.
//! * **Bounded memory** — captured output is capped; overflow is still
//!   *read* (to let the child finish) but discarded, and reported via
//!   [`ProcessResult::truncated`].
//! * **Exit codes are preserved** — never reported as success when non-zero.
//! * **Launch failures are structured** — a missing executable becomes
//!   `spawn_error`, not a panic.
//! * **Cancellation** — a [`CancelToken`] kills the child; the token can be
//!   wired to a UI button or a future service request.
//! * **Timeouts** — a watchdog kills children that hang (used for version
//!   probes; available for any call).
//! * **No shell** — programs are spawned directly; arguments are passed
//!   verbatim, so paths with spaces need no quoting.
//!
//! Known limitation: the watchdog kills the **direct child** only. If the
//! child spawns grandchildren that inherit the output pipes, EOF on the
//! pipes may be delayed until those grandchildren exit; Pico still reports
//! the child's termination immediately.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// Default cap for captured stdout/stderr, per stream: 10 MiB.
pub const DEFAULT_MAX_OUTPUT_BYTES: usize = 10 * 1024 * 1024;

/// Cooperative cancellation token.
#[derive(Debug, Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn new() -> CancelToken {
        CancelToken(Arc::new(AtomicBool::new(false)))
    }

    /// Requests cancellation; the watchdog kills the child on the next poll.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// Whether to capture a child's output or let it inherit the parent's stdio.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StdioMode {
    /// Capture into memory (JSON mode, tests).
    Capture,
    /// Inherit the parent's stdio (interactive text-mode `run`).
    Inherit,
}

/// Fully specified child process invocation.
#[derive(Debug, Clone)]
pub struct ProcessSpec {
    /// Program to run: an absolute/relative path, or a bare name that is
    /// resolved against `search_paths` (see [`run_process`]).
    pub program: String,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    /// Extra environment variables applied on top of the inherited
    /// environment.
    pub env: BTreeMap<String, String>,
    pub stdio: StdioMode,
    pub timeout: Option<Duration>,
    pub max_output: usize,
    /// Directories used to resolve bare program names. Empty means "use the
    /// process PATH". Kept explicit so tests never need to mutate global env.
    pub search_paths: Vec<PathBuf>,
}

impl ProcessSpec {
    pub fn new(program: impl Into<String>) -> ProcessSpec {
        ProcessSpec {
            program: program.into(),
            args: Vec::new(),
            cwd: None,
            env: BTreeMap::new(),
            stdio: StdioMode::Capture,
            timeout: None,
            max_output: DEFAULT_MAX_OUTPUT_BYTES,
            search_paths: Vec::new(),
        }
    }

    pub fn arg(mut self, arg: impl Into<String>) -> ProcessSpec {
        self.args.push(arg.into());
        self
    }

    pub fn args(mut self, args: &[String]) -> ProcessSpec {
        self.args.extend(args.iter().cloned());
        self
    }

    pub fn cwd(mut self, cwd: impl Into<PathBuf>) -> ProcessSpec {
        self.cwd = Some(cwd.into());
        self
    }

    pub fn env(mut self, key: impl Into<String>, value: impl Into<String>) -> ProcessSpec {
        self.env.insert(key.into(), value.into());
        self
    }

    pub fn stdio(mut self, mode: StdioMode) -> ProcessSpec {
        self.stdio = mode;
        self
    }

    pub fn timeout(mut self, timeout: Duration) -> ProcessSpec {
        self.timeout = Some(timeout);
        self
    }

    pub fn max_output(mut self, max_output: usize) -> ProcessSpec {
        self.max_output = max_output;
        self
    }

    pub fn search_paths(mut self, paths: Vec<PathBuf>) -> ProcessSpec {
        self.search_paths = paths;
        self
    }

    /// argv as printable strings, for logs and reports.
    pub fn command_line(&self) -> Vec<String> {
        let mut line = vec![self.program.clone()];
        line.extend(self.args.iter().cloned());
        line
    }
}

/// Outcome of running a child process. Never panics for ordinary failures.
#[derive(Debug, Clone)]
pub struct ProcessResult {
    /// The program as it was finally invoked (resolved path when possible).
    pub program: String,
    pub args: Vec<String>,
    /// `None` when the child could not be launched, was killed by the
    /// watchdog / cancel token, or was terminated by a signal.
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub duration: Duration,
    pub cancelled: bool,
    pub timed_out: bool,
    pub truncated: bool,
    /// Structured launch failure (executable missing, permission denied, ...).
    pub spawn_error: Option<String>,
}

impl ProcessResult {
    /// True only when the child launched and exited with code 0.
    pub fn success(&self) -> bool {
        self.spawn_error.is_none()
            && !self.cancelled
            && !self.timed_out
            && self.exit_code == Some(0)
    }
}

/// Splits the current `PATH` into directories.
pub fn path_dirs() -> Vec<PathBuf> {
    std::env::var_os("PATH")
        .map(|paths| {
            std::env::split_paths(&paths)
                .filter(|p| !p.as_os_str().is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// Executable candidate extensions per platform (in probe order).
#[cfg(windows)]
const EXECUTABLE_EXTENSIONS: &[&str] = &["", ".exe", ".cmd", ".bat"];
#[cfg(not(windows))]
const EXECUTABLE_EXTENSIONS: &[&str] = &[""];

fn is_executable_file(path: &Path) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

/// Resolves a program name against explicit search directories.
///
/// Absolute and relative paths with a separator are used as-is (still checked
/// for existence). Bare names are probed per directory with the platform's
/// executable extensions (`.exe`, `.cmd`, `.bat` on Windows).
pub fn resolve_in(dirs: &[PathBuf], name: &str) -> Option<PathBuf> {
    let candidate_path = Path::new(name);
    let looks_like_path = candidate_path.is_absolute()
        || name.contains('/')
        || name.contains('\\')
        || candidate_path
            .parent()
            .is_some_and(|p| !p.as_os_str().is_empty());
    if looks_like_path {
        return is_executable_file(candidate_path).then(|| candidate_path.to_path_buf());
    }
    for dir in dirs {
        for ext in EXECUTABLE_EXTENSIONS {
            let candidate = dir.join(format!("{name}{ext}"));
            if is_executable_file(&candidate) {
                return Some(candidate);
            }
        }
    }
    None
}

/// Resolves a program name: `spec.search_paths` when set, otherwise `PATH`.
pub fn resolve_program(spec: &ProcessSpec) -> Option<PathBuf> {
    if spec.search_paths.is_empty() {
        resolve_in(&path_dirs(), &spec.program)
    } else {
        resolve_in(&spec.search_paths, &spec.program)
    }
}

fn spawn_error_message(program: &str, error: &std::io::Error) -> String {
    match error.kind() {
        std::io::ErrorKind::NotFound => {
            format!("program not found: '{program}' is not on PATH or does not exist")
        }
        std::io::ErrorKind::PermissionDenied => {
            format!("permission denied while launching '{program}'")
        }
        _ => format!("failed to launch '{program}': {error}"),
    }
}

/// Runs a child process to completion.
///
/// Bare program names are resolved against the search paths (or `PATH`).
/// Output is drained concurrently to avoid pipe deadlocks; the watchdog loop
/// honours [`CancelToken`] and [`ProcessSpec::timeout`].
pub fn run_process(spec: &ProcessSpec, cancel: &CancelToken) -> ProcessResult {
    let started = Instant::now();
    let base = ProcessResult {
        program: spec.program.clone(),
        args: spec.args.clone(),
        exit_code: None,
        stdout: String::new(),
        stderr: String::new(),
        duration: Duration::ZERO,
        cancelled: false,
        timed_out: false,
        truncated: false,
        spawn_error: None,
    };

    if cancel.is_cancelled() {
        return ProcessResult {
            cancelled: true,
            duration: started.elapsed(),
            ..base
        };
    }

    let resolved = resolve_program(spec);
    let Some(program_path) = resolved else {
        return ProcessResult {
            spawn_error: Some(spawn_error_message(
                &spec.program,
                &std::io::Error::new(std::io::ErrorKind::NotFound, "not found"),
            )),
            duration: started.elapsed(),
            ..base
        };
    };
    // Report form of the resolved program: serialized through the shared
    // path rules so Windows verbatim prefixes never leak into reports.
    let program_display = raven_core::path_to_string(&program_path);

    let mut command = Command::new(&program_path);
    command.args(&spec.args).stdin(Stdio::null());
    if let Some(cwd) = &spec.cwd {
        command.current_dir(cwd);
    }
    for (key, value) in &spec.env {
        command.env(key, value);
    }
    match spec.stdio {
        StdioMode::Capture => {
            command.stdout(Stdio::piped()).stderr(Stdio::piped());
        }
        StdioMode::Inherit => {
            command.stdout(Stdio::inherit()).stderr(Stdio::inherit());
        }
    }

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(err) => {
            return ProcessResult {
                program: program_display,
                spawn_error: Some(spawn_error_message(&spec.program, &err)),
                duration: started.elapsed(),
                ..base
            };
        }
    };

    // Drain pipes concurrently with the wait loop so that:
    //   * a full pipe can never deadlock the child, and
    //   * cancellation/timeout fire even while output is pending.
    let mut stdout_pipe = child.stdout.take();
    let mut stderr_pipe = child.stderr.take();
    let mut cancelled = false;
    let mut timed_out = false;
    let (status, out, err) = std::thread::scope(|scope| {
        let out_handle = scope.spawn(|| read_capped(&mut stdout_pipe, spec.max_output));
        let err_handle = scope.spawn(|| read_capped(&mut stderr_pipe, spec.max_output));
        let wait_handle = scope.spawn(|| {
            wait_with_watchdog(
                &mut child,
                cancel,
                spec.timeout,
                &mut cancelled,
                &mut timed_out,
            )
        });
        let status = wait_handle.join().unwrap_or(None);
        let out = out_handle.join().unwrap_or((String::new(), false));
        let err = err_handle.join().unwrap_or((String::new(), false));
        (status, out, err)
    });

    ProcessResult {
        program: program_display,
        args: spec.args.clone(),
        exit_code: status.as_ref().and_then(|status| status.code()),
        stdout: out.0,
        stderr: err.0,
        duration: started.elapsed(),
        cancelled,
        timed_out,
        truncated: out.1 || err.1,
        spawn_error: None,
    }
}

/// Waits for the child, polling the cancel token and the timeout.
fn wait_with_watchdog(
    child: &mut Child,
    cancel: &CancelToken,
    timeout: Option<Duration>,
    cancelled: &mut bool,
    timed_out: &mut bool,
) -> Option<std::process::ExitStatus> {
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Some(status),
            Ok(None) => {}
            Err(_) => return None,
        }
        if cancel.is_cancelled() {
            *cancelled = true;
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        if let Some(limit) = timeout {
            if started.elapsed() >= limit {
                *timed_out = true;
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Reads a stream up to `cap` bytes; keeps draining (discarding) beyond the
/// cap so the child can finish, and reports truncation.
fn read_capped(stream: &mut Option<impl Read>, cap: usize) -> (String, bool) {
    let Some(stream) = stream.as_mut() else {
        return (String::new(), false);
    };
    let mut bytes: Vec<u8> = Vec::new();
    let mut truncated = false;
    let mut chunk = [0u8; 8192];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                if bytes.len() < cap {
                    let remaining = cap - bytes.len();
                    let take = n.min(remaining);
                    bytes.extend_from_slice(&chunk[..take]);
                    if take < n {
                        truncated = true;
                    }
                } else {
                    truncated = true;
                }
            }
            Err(_) => break,
        }
    }
    (String::from_utf8_lossy(&bytes).into_owned(), truncated)
}

#[cfg(test)]
mod tests;
