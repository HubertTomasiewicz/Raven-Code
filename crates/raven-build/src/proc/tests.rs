//! Unit tests for the process-execution layer, using fake executables so no
//! real toolchain is required.

use std::time::Duration;

use raven_core::progress::{ProgressSink, SilentSink};
use testkit::FakeTool;

use crate::proc::{CancelToken, ProcessSpec, StdioMode, path_dirs, resolve_in, run_process};

fn spec(tool: &FakeTool, args: &[&str]) -> ProcessSpec {
    let mut spec =
        ProcessSpec::new(tool.program().display().to_string()).search_paths(tool.search_paths());
    for arg in args {
        spec = spec.arg(*arg);
    }
    // Route the fake tool's invocation log through the process env, exactly
    // like a real consumer would pass extra env to a child.
    let (key, value) = tool.log_env();
    spec.env(key, value)
}

#[test]
fn captures_stdout_and_exit_code() {
    let tool = FakeTool::new("echo");
    let result = run_process(&spec(&tool, &[]), &CancelToken::new());
    assert!(result.success(), "expected success, got {result:?}");
    assert_eq!(result.exit_code, Some(0));
    assert!(result.stdout.contains("fake echo ok"));
    assert!(result.duration > Duration::ZERO);
}

#[test]
fn preserves_nonzero_exit_codes() {
    let tool = FakeTool::new("failing");
    let mut process_spec = spec(&tool, &[]);
    process_spec
        .env
        .insert("FAKE_TOOL_EXIT".to_string(), "3".to_string());
    let result = run_process(&process_spec, &CancelToken::new());
    assert!(!result.success());
    assert_eq!(result.exit_code, Some(3));
}

#[test]
fn missing_executable_is_a_structured_spawn_error() {
    let empty_dir = tempfile::tempdir().unwrap();
    let spec = ProcessSpec::new("definitely-not-a-real-tool-42")
        .search_paths(vec![empty_dir.path().to_path_buf()]);
    let result = run_process(&spec, &CancelToken::new());
    assert!(!result.success());
    assert!(result.spawn_error.as_deref().unwrap().contains("not found"));
    assert_eq!(result.exit_code, None);
}

#[test]
fn args_with_spaces_are_passed_verbatim() {
    let tool = FakeTool::new("args");
    let result = run_process(&spec(&tool, &["hello world", "a\"b"]), &CancelToken::new());
    assert!(result.success());
    let logged = tool.log_lines().join("\n");
    assert!(logged.contains("hello world"), "log: {logged}");
    assert!(logged.contains("a\"b"), "log: {logged}");
}

#[test]
fn cwd_is_honoured() {
    let tool = FakeTool::with_body("pwdtool", if cfg!(windows) { "echo %CD%" } else { "pwd" });
    let workdir = tempfile::tempdir().unwrap();
    let spec = spec(&tool, &[]).cwd(workdir.path());
    let result = run_process(&spec, &CancelToken::new());
    assert!(result.success());
    assert!(
        result
            .stdout
            .contains(workdir.path().to_string_lossy().as_ref()),
        "stdout: {}",
        result.stdout
    );
}

#[test]
fn timeout_kills_hanging_children() {
    let tool = FakeTool::sleeping("slow", 30);
    let spec = spec(&tool, &[]).timeout(Duration::from_millis(400));
    let result = run_process(&spec, &CancelToken::new());
    assert!(result.timed_out);
    assert!(!result.success());
    assert!(
        result.duration < Duration::from_secs(10),
        "kill took too long"
    );
}

#[test]
fn cancellation_kills_the_child() {
    let tool = FakeTool::sleeping("slow", 30);
    let cancel = CancelToken::new();
    let cancel_clone = cancel.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(200));
        cancel_clone.cancel();
    });
    let result = run_process(&spec(&tool, &[]), &cancel);
    assert!(result.cancelled);
    assert!(!result.success());
    assert!(result.duration < Duration::from_secs(10));
}

#[test]
fn cancellation_before_spawn_returns_immediately() {
    let tool = FakeTool::new("never");
    let cancel = CancelToken::new();
    cancel.cancel();
    let result = run_process(&spec(&tool, &[]), &cancel);
    assert!(result.cancelled);
    assert_eq!(result.exit_code, None);
}

#[test]
fn oversized_output_is_capped_and_flagged() {
    let tool = FakeTool::noisy("noisy", 5000);
    let spec = spec(&tool, &[]).max_output(1024);
    let result = run_process(&spec, &CancelToken::new());
    assert!(result.truncated);
    assert!(
        result.stdout.len() <= 1024,
        "captured {} bytes",
        result.stdout.len()
    );
    // The process still completes normally despite the cap.
    assert!(result.success());
}

#[test]
fn inherit_mode_reports_exit_code_without_capture() {
    let tool = FakeTool::new("inherited");
    let spec = spec(&tool, &[]).stdio(StdioMode::Inherit);
    let result = run_process(&spec, &CancelToken::new());
    assert!(result.success());
    assert_eq!(result.stdout, String::new());
}

#[test]
fn resolve_in_finds_executables_and_respects_order() {
    let tool = FakeTool::new("resolved");
    let dirs = tool.search_paths();
    let found = resolve_in(&dirs, tool.name()).expect("should resolve");
    assert_eq!(found, tool.program());

    let empty = tempfile::tempdir().unwrap();
    assert!(resolve_in(&[empty.path().to_path_buf()], tool.name()).is_none());
    // Absolute path input works regardless of dirs.
    let program = tool.program();
    assert!(resolve_in(&[], &program.display().to_string()).is_some());
}

#[test]
fn path_dirs_is_non_empty_in_practice() {
    // A normal test environment always has PATH set; this guards against
    // regressions that would silently produce an empty search list.
    let dirs = path_dirs();
    assert!(!dirs.is_empty() || std::env::var_os("PATH").is_none());
}

#[test]
fn spec_command_line_is_stable() {
    let spec = ProcessSpec::new("cargo").arg("build").arg("--release");
    assert_eq!(spec.command_line(), vec!["cargo", "build", "--release"]);
}

#[test]
fn search_paths_take_precedence_over_global_path() {
    // A fake `python` in an isolated directory must win over any real python
    // installed on the machine: search paths are authoritative.
    let tool = FakeTool::new("python");
    let spec = ProcessSpec::new("python").search_paths(tool.search_paths());
    let result = run_process(&spec, &CancelToken::new());
    assert!(result.success());
    assert!(result.stdout.contains("fake python ok"));
}

#[test]
fn silent_sink_is_available() {
    // Ensures the SilentSink import stays used across platforms.
    SilentSink.on_event(&raven_core::progress::ProgressEvent::Stage {
        message: "noop".into(),
    });
}
