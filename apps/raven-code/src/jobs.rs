//! Background process jobs: build / test / run / arbitrary commands.
//!
//! Everything executes through the **Raven Pico SDK** (`raven-build`): the
//! same hardened process layer the CLI uses — captured output capped at
//! 10 MiB per stream, no shell interpretation, structured launch errors,
//! cancellation via `CancelToken` and progress via `ProgressSink`.
//!
//! The editor UI never blocks: each job runs on a plain worker thread and
//! streams output into a bounded channel that `app.rs` drains every frame
//! (no polling loops, no unbounded accumulation).

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use raven_build::{CancelToken, Op, OpContext, Profile, StdioMode};
use raven_core::progress::{ProgressEvent, ProgressSink};

/// Hard cap on retained output lines per job (older lines are dropped).
const OUTPUT_LINE_CAP: usize = 5000;

static NEXT_JOB_ID: AtomicU64 = AtomicU64::new(1);

/// One line of job output as shown in the Output panel.
#[allow(dead_code)] // `job_id` supports per-job filtering in future versions
#[derive(Debug, Clone)]
pub struct OutputLine {
    pub job_id: u64,
    pub stream: Stream,
    pub text: String,
    pub at: Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stream {
    Info,
    Stdout,
    Stderr,
    Error,
}

/// Lifecycle status mirrored in the Output panel header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobStatus {
    Running,
    Succeeded,
    Failed,
    Cancelled,
    LaunchFailed,
}

/// Shared output buffer the worker threads append to.
#[derive(Clone, Default)]
pub struct OutputBuffer {
    inner: Arc<Mutex<OutputInner>>,
}

#[derive(Default)]
struct OutputInner {
    lines: Vec<OutputLine>,
    jobs: Vec<JobSnapshot>,
}

/// Cheap status snapshot kept in the shared buffer so the UI can render
/// job state even after the worker finished.
#[derive(Debug, Clone)]
pub struct JobSnapshot {
    pub id: u64,
    pub title: String,
    pub status: JobStatus,
    pub exit_code: Option<i32>,
}

impl OutputBuffer {
    pub fn new() -> OutputBuffer {
        OutputBuffer::default()
    }

    fn push_line(&self, line: OutputLine) {
        let mut inner = self.inner.lock().unwrap();
        inner.lines.push(line);
        if inner.lines.len() > OUTPUT_LINE_CAP {
            let overflow = inner.lines.len() - OUTPUT_LINE_CAP;
            inner.lines.drain(..overflow);
        }
    }

    fn update_job(&self, id: u64, status: JobStatus, exit_code: Option<i32>) {
        let mut inner = self.inner.lock().unwrap();
        if let Some(snapshot) = inner.jobs.iter_mut().find(|job| job.id == id) {
            snapshot.status = status;
            snapshot.exit_code = exit_code;
        }
    }

    pub fn add_job(&self, id: u64, title: &str) {
        let mut inner = self.inner.lock().unwrap();
        inner.jobs.push(JobSnapshot {
            id,
            title: title.to_string(),
            status: JobStatus::Running,
            exit_code: None,
        });
        if inner.jobs.len() > 50 {
            inner.jobs.remove(0);
        }
    }

    pub fn clear(&self) {
        let mut inner = self.inner.lock().unwrap();
        inner.lines.clear();
        inner.jobs.clear();
    }

    /// Snapshot for rendering (cheap clone of the bounded buffer).
    pub fn snapshot(&self) -> (Vec<OutputLine>, Vec<JobSnapshot>) {
        let inner = self.inner.lock().unwrap();
        (inner.lines.clone(), inner.jobs.clone())
    }
}

/// The UI-side job manager. `app` polls `drain_messages` each frame.
pub struct JobManager {
    pub buffer: OutputBuffer,
    messages: std::sync::mpsc::Receiver<JobMessage>,
    sender: Sender<JobMessage>,
    running: Vec<(u64, CancelToken)>,
    pub last_job_id: u64,
}

enum JobMessage {
    Done {
        job_id: u64,
        status: JobStatus,
        exit_code: Option<i32>,
    },
}

impl Default for JobManager {
    fn default() -> Self {
        Self::new()
    }
}

impl JobManager {
    pub fn new() -> JobManager {
        let (sender, receiver) = std::sync::mpsc::channel();
        JobManager {
            buffer: OutputBuffer::new(),
            messages: receiver,
            sender,
            running: Vec::new(),
            last_job_id: 0,
        }
    }

    /// Drains completion messages from worker threads (non-blocking).
    pub fn drain_messages(&mut self) {
        while let Ok(message) = self.messages.try_recv() {
            match message {
                JobMessage::Done {
                    job_id,
                    status,
                    exit_code,
                } => {
                    self.buffer.update_job(job_id, status, exit_code);
                    if let Some(slot) = self.running.iter_mut().find(|(id, _)| *id == job_id) {
                        // Keep the token until cancel is impossible.
                        let _ = slot;
                    }
                    self.running.retain(|(id, _)| *id != job_id);
                }
            }
        }
    }

    /// Cancels every running job (wired to the Output panel stop button and
    /// the CancelJob action).
    pub fn cancel_all(&self) {
        for (_, cancel) in &self.running {
            cancel.cancel();
        }
    }

    pub fn any_running(&self) -> bool {
        !self.running.is_empty()
    }

    /// Spawns a Raven Pico SDK operation (build/test/run/clean) on a worker
    /// thread. Detection runs on the worker too — scanning the filesystem
    /// from the UI thread is exactly what we must not do.
    pub fn start_pico_operation(
        &mut self,
        root: PathBuf,
        op: Op,
        release: bool,
        extra_args: Vec<String>,
    ) {
        let id = NEXT_JOB_ID.fetch_add(1, Ordering::Relaxed);
        let title = format!("{} — {}", op.as_str(), raven_core::path_to_string(&root));
        self.last_job_id = id;
        self.buffer.add_job(id, &title);
        self.push(
            id,
            Stream::Info,
            format!("$ raven-pico {} {}", op.as_str(), title),
        );

        let sender = self.sender.clone();
        let buffer = self.buffer.clone();
        let cancel = CancelToken::new();
        self.running.push((id, cancel.clone()));

        std::thread::Builder::new()
            .name(format!("raven-code-job-{id}"))
            .spawn(move || {
                let outcome =
                    run_pico_operation(root, op, release, extra_args, cancel, buffer.clone(), id);
                let (status, exit_code) = match outcome {
                    Ok(success) => {
                        if success {
                            (JobStatus::Succeeded, Some(0))
                        } else {
                            (JobStatus::Failed, Some(1))
                        }
                    }
                    Err(error) => {
                        buffer.push_line(OutputLine {
                            job_id: id,
                            stream: Stream::Error,
                            text: format!("error: {error}"),
                            at: Instant::now(),
                        });
                        (JobStatus::LaunchFailed, None)
                    }
                };
                let _ = sender.send(JobMessage::Done {
                    job_id: id,
                    status,
                    exit_code,
                });
            })
            .expect("spawn job thread");
    }

    /// Runs an arbitrary command (Run Command…), still through the Pico
    /// process layer: no shell, args passed verbatim.
    pub fn start_command(&mut self, program: String, args: Vec<String>, cwd: PathBuf) {
        let id = NEXT_JOB_ID.fetch_add(1, Ordering::Relaxed);
        let title = format!("{} {}", program, args.join(" "));
        self.last_job_id = id;
        self.buffer.add_job(id, &title);
        self.push(id, Stream::Info, format!("$ {title}"));

        let sender = self.sender.clone();
        let buffer = self.buffer.clone();
        let cancel = CancelToken::new();
        self.running.push((id, cancel.clone()));

        std::thread::Builder::new()
            .name(format!("raven-code-cmd-{id}"))
            .spawn(move || {
                let mut spec = raven_build::ProcessSpec::new(program);
                spec.args = args;
                spec.cwd = Some(cwd);
                let result = raven_build::proc::run_process(&spec, &cancel);
                for line in result.stdout.lines() {
                    buffer.push_line(OutputLine {
                        job_id: id,
                        stream: Stream::Stdout,
                        text: line.to_string(),
                        at: Instant::now(),
                    });
                }
                for line in result.stderr.lines() {
                    buffer.push_line(OutputLine {
                        job_id: id,
                        stream: Stream::Stderr,
                        text: line.to_string(),
                        at: Instant::now(),
                    });
                }
                let status = if result.cancelled {
                    JobStatus::Cancelled
                } else if result.success() {
                    JobStatus::Succeeded
                } else {
                    JobStatus::Failed
                };
                buffer.push_line(OutputLine {
                    job_id: id,
                    stream: Stream::Info,
                    text: format!(
                        "[exit code: {}{}]",
                        result
                            .exit_code
                            .map(|code| code.to_string())
                            .unwrap_or_else(|| "?".into()),
                        if result.cancelled { ", cancelled" } else { "" }
                    ),
                    at: Instant::now(),
                });
                let _ = sender.send(JobMessage::Done {
                    job_id: id,
                    status,
                    exit_code: result.exit_code,
                });
            })
            .expect("spawn command thread");
    }

    fn push(&self, job_id: u64, stream: Stream, text: String) {
        self.buffer.push_line(OutputLine {
            job_id,
            stream,
            text,
            at: Instant::now(),
        });
    }
}

/// Detects and executes one Pico operation; returns success flag.
fn run_pico_operation(
    root: PathBuf,
    op: Op,
    release: bool,
    extra_args: Vec<String>,
    cancel: CancelToken,
    buffer: OutputBuffer,
    job_id: u64,
) -> Result<bool, String> {
    let detection = raven_projects::detect(&root, &raven_projects::DetectOptions::default())
        .map_err(|error| error.to_string())?;
    let (kind, project_root) =
        raven_projects::select(&detection, None).map_err(|error| error.to_string())?;
    let config = match &detection.config {
        Some(loaded) => Some(loaded.require().map_err(|error| error.to_string())?.clone()),
        None => None,
    };

    buffer.push_line(OutputLine {
        job_id,
        stream: Stream::Info,
        text: format!(
            "detected {} project at {}",
            kind.display_name(),
            raven_core::path_to_string(&project_root)
        ),
        at: Instant::now(),
    });

    let sink = BufferSink {
        job_id,
        buffer: buffer.clone(),
    };

    let ctx = OpContext {
        root: project_root,
        kind,
        config: config.as_ref(),
        profile: if release {
            Profile::Release
        } else {
            Profile::Debug
        },
        extra_args,
        env: BTreeMap::new(),
        search_paths: raven_build::proc::path_dirs(),
        sink: Some(&sink),
        cancel,
        stdio: StdioMode::Capture,
    };

    let outcome = raven_build::execute_operation(&ctx, op).map_err(|error| error.to_string())?;
    let success = outcome.success();
    if let Some(result) = &outcome.result {
        for line in result.stdout.lines() {
            buffer.push_line(OutputLine {
                job_id,
                stream: Stream::Stdout,
                text: line.to_string(),
                at: Instant::now(),
            });
        }
        for line in result.stderr.lines() {
            buffer.push_line(OutputLine {
                job_id,
                stream: Stream::Stderr,
                text: line.to_string(),
                at: Instant::now(),
            });
        }
        if result.truncated {
            buffer.push_line(OutputLine {
                job_id,
                stream: Stream::Info,
                text: "[output truncated at 10 MiB per stream]".to_string(),
                at: Instant::now(),
            });
        }
    }
    Ok(success && !outcome.cancelled())
}

/// Streams Pico progress events into the output buffer.
struct BufferSink {
    job_id: u64,
    buffer: OutputBuffer,
}

impl ProgressSink for BufferSink {
    fn on_event(&self, event: &ProgressEvent) {
        let text = match event {
            ProgressEvent::Stage { message } => format!("== {message}"),
            ProgressEvent::ToolResolved { tool, version } => match version {
                Some(version) => format!("tool: {tool} {version}"),
                None => format!("tool: {tool}"),
            },
            ProgressEvent::CommandSpawned { program, args } => {
                format!("$ {} {}", program, args.join(" "))
            }
            ProgressEvent::CommandFinished {
                exit_code,
                duration_ms,
            } => {
                let code = exit_code
                    .map(|code| code.to_string())
                    .unwrap_or_else(|| "?".to_string());
                format!("[process finished: exit {code}, {duration_ms} ms]")
            }
            ProgressEvent::Warning { message } => format!("warning: {message}"),
        };
        self.buffer.push_line(OutputLine {
            job_id: self.job_id,
            stream: Stream::Info,
            text,
            at: Instant::now(),
        });
    }
}
