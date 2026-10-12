//! Test utilities shared by all Raven Pico crates: project fixtures and
//! fake toolchains.
//!
//! Fake tools are real executables (shell scripts on Unix, `.cmd` files on
//! Windows) so tests exercise the exact process-execution paths used in
//! production without requiring any toolchain to be installed.
//!
//! Contract of every fake tool:
//!
//! * appends `fake <name> <argv...>` to the file named by `FAKE_TOOL_LOG`
//!   (when that variable is set),
//! * prints `fake <name> ok` to stdout,
//! * exits with `FAKE_TOOL_EXIT` (when set) or `0`.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use tempfile::TempDir;

/// Writes `content` to `dir/rel_path`, creating parent directories.
pub fn write_file(dir: &Path, rel_path: &str, content: &str) -> io::Result<()> {
    let target = dir.join(rel_path);
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(target, content)
}

/// Creates marker files for detection fixtures (all empty except given content).
pub fn marker_project(dir: &Path, markers: &[(&str, &str)]) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    for (name, content) in markers {
        write_file(dir, name, content)?;
    }
    Ok(())
}

const UNIX_TOOL_TEMPLATE: &str = r#"#!/bin/sh
invocation="fake {name} $*"
if [ -z "$*" ]; then invocation="fake {name}"; fi
if [ -n "$FAKE_TOOL_LOG" ]; then
  printf '%s\n' "$invocation" >> "$FAKE_TOOL_LOG"
fi
{body}
if [ -n "$FAKE_TOOL_EXIT" ]; then
  exit "$FAKE_TOOL_EXIT"
fi
exit 0
"#;

const WINDOWS_TOOL_TEMPLATE: &str = r#"@echo off
if defined FAKE_TOOL_LOG (
  if "%~1"=="" (echo fake {name}>> "%FAKE_TOOL_LOG%") else (echo fake {name} %*>> "%FAKE_TOOL_LOG%")
)
{body}
if defined FAKE_TOOL_EXIT exit /b %FAKE_TOOL_EXIT%
exit /b 0
"#;

/// A fake executable plus its invocation log.
pub struct FakeTool {
    _dir: TempDir,
    name: String,
    pub log: PathBuf,
    program: PathBuf,
}

impl FakeTool {
    /// Creates a fake tool that echoes its arguments and exits 0.
    pub fn new(name: &str) -> FakeTool {
        FakeTool::with_body(name, "echo \"fake {name} ok\"")
    }

    /// Creates a fake tool with a custom script body. `{name}` in the body is
    /// substituted; the script's own stdout is whatever the body prints.
    pub fn with_body(name: &str, body: &str) -> FakeTool {
        FakeTool::build(name, body).expect("failed to create fake tool")
    }

    /// Creates a fake tool that sleeps for `seconds` then prints `slept`.
    ///
    /// On Unix the sleep uses `exec` so the process that gets killed IS the
    /// sleeper (no orphaned grandchild holding the pipes open).
    pub fn sleeping(name: &str, seconds: u64) -> FakeTool {
        #[cfg(unix)]
        let body = format!("exec sleep {seconds}");
        #[cfg(windows)]
        let body = format!("ping -n {n} 127.0.0.1 >nul\necho slept", n = seconds + 1);
        FakeTool::with_body(name, &body)
    }

    /// Creates a fake tool that prints many lines (for truncation tests).
    pub fn noisy(name: &str, lines: usize) -> FakeTool {
        #[cfg(unix)]
        let body =
            format!("i=0\nwhile [ $i -lt {lines} ]; do echo 0123456789ABCDEF; i=$((i+1)); done");
        #[cfg(windows)]
        let body = format!("for /l %%i in (1,1,{lines}) do @echo 0123456789ABCDEF0123456789ABCDEF");
        FakeTool::with_body(name, &body)
    }

    fn build(name: &str, body: &str) -> io::Result<FakeTool> {
        let dir = TempDir::new()?;
        let body = body.replace("{name}", name);
        let (program, script) = if cfg!(windows) {
            let script = WINDOWS_TOOL_TEMPLATE
                .replace("{name}", name)
                .replace("{body}", &body);
            (dir.path().join(format!("{name}.cmd")), script)
        } else {
            let script = UNIX_TOOL_TEMPLATE
                .replace("{name}", name)
                .replace("{body}", &body);
            let program = dir.path().join(name);
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::write(&program, &script)?;
                fs::set_permissions(&program, fs::Permissions::from_mode(0o755))?;
            }
            #[cfg(not(unix))]
            fs::write(&program, &script)?;
            (program, script)
        };
        let _ = script; // kept for clarity; content already written above
        let log = dir.path().join("invocations.log");
        Ok(FakeTool {
            _dir: dir,
            name: name.to_string(),
            log,
            program,
        })
    }

    /// Full path of the fake executable.
    pub fn program(&self) -> &Path {
        &self.program
    }

    /// Search-path list containing (only) the tool's directory.
    pub fn search_paths(&self) -> Vec<PathBuf> {
        vec![
            self.program
                .parent()
                .expect("tool has parent")
                .to_path_buf(),
        ]
    }

    /// The `(FAKE_TOOL_LOG, path)` env pair to pass to spawned processes.
    pub fn log_env(&self) -> (String, String) {
        ("FAKE_TOOL_LOG".to_string(), self.log.display().to_string())
    }

    /// Lines recorded in the invocation log.
    pub fn log_lines(&self) -> Vec<String> {
        fs::read_to_string(&self.log)
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }

    pub fn name(&self) -> &str {
        &self.name
    }
}

/// Builds the default search paths for tests: only `dirs`, so tests never
/// depend on the real machine PATH.
pub fn isolated_search_paths(dirs: &[PathBuf]) -> Vec<PathBuf> {
    dirs.to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fake_tool_records_invocations() {
        let tool = FakeTool::new("cargo");
        let spec_log = tool.log_env();
        let output = std::process::Command::new(tool.program())
            .arg("build")
            .arg("--release")
            .arg("hello world")
            .env(&spec_log.0, &spec_log.1)
            .output()
            .expect("spawn fake tool");
        assert!(output.status.success());
        let lines = tool.log_lines();
        assert_eq!(lines, vec!["fake cargo build --release hello world"]);
        assert_eq!(
            String::from_utf8_lossy(&output.stdout).trim(),
            "fake cargo ok"
        );
    }

    #[test]
    fn fake_tool_honours_exit_code_env() {
        let tool = FakeTool::new("failing");
        let output = std::process::Command::new(tool.program())
            .env("FAKE_TOOL_EXIT", "3")
            .output()
            .expect("spawn fake tool");
        assert_eq!(output.status.code(), Some(3));
    }

    #[test]
    fn marker_project_writes_files() {
        let dir = TempDir::new().unwrap();
        marker_project(dir.path(), &[("Cargo.toml", "[package]\n")]).unwrap();
        assert!(dir.path().join("Cargo.toml").is_file());
    }
}
