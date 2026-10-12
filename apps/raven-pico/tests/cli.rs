//! End-to-end CLI tests: run the real binary against fixture projects using
//! fake toolchains. No real compiler is required.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;
use tempfile::TempDir;

fn binary() -> &'static str {
    env!("CARGO_BIN_EXE_raven-pico")
}

struct DirGuard(TempDir);

impl DirGuard {
    fn project_dir(&self) -> PathBuf {
        self.0.path().join("proj")
    }
}

fn make_project(files: &[(&str, &str)]) -> DirGuard {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path().join("proj");
    fs::create_dir_all(&dir).unwrap();
    for (name, content) in files {
        fs::write(dir.join(name), content).unwrap();
    }
    DirGuard(tmp)
}

fn make_project_with_name(name: &str, files: &[(&str, &str)]) -> (TempDir, PathBuf) {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path().join(name);
    fs::create_dir_all(&dir).unwrap();
    for (file, content) in files {
        fs::write(dir.join(file), content).unwrap();
    }
    (tmp, dir)
}

/// Runs the CLI with an isolated PATH: `extra_path` is PREPENDED so fake
/// tools win over any real toolchain installed on the machine.
fn run_cli(args: &[&str], dir: &Path, extra_path: Option<&Path>, envs: &[(&str, &str)]) -> Output {
    let mut command = Command::new(binary());
    command.args(args).current_dir(dir);
    let real_path = std::env::var_os("PATH").unwrap_or_default();
    if let Some(extra) = extra_path {
        let combined = std::env::join_paths(
            std::iter::once(extra.to_path_buf()).chain(std::env::split_paths(&real_path)),
        )
        .unwrap();
        command.env("PATH", combined);
    } else {
        let combined = std::env::join_paths(std::env::split_paths(&real_path)).unwrap();
        command.env("PATH", combined);
    }
    for (key, value) in envs {
        command.env(key, value);
    }
    command.output().expect("spawn raven-pico")
}

fn stdout_json(output: &Output) -> Value {
    let text = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(&text)
        .unwrap_or_else(|error| panic!("stdout is not valid JSON: {error}\n---\n{text}"))
}

// ------------------------------------------------------------ basics ----

#[test]
fn version_flag_prints_semver() {
    let tmp = TempDir::new().unwrap();
    let output = run_cli(&["--version"], tmp.path(), None, &[]);
    assert!(output.status.success());
    let text = String::from_utf8_lossy(&output.stdout);
    assert_eq!(text.trim(), "raven-pico 0.2.0");
}

#[test]
fn help_lists_all_documented_commands() {
    let tmp = TempDir::new().unwrap();
    let output = run_cli(&["--help"], tmp.path(), None, &[]);
    assert!(output.status.success());
    let text = String::from_utf8_lossy(&output.stdout);
    for command in [
        "detect",
        "build",
        "test",
        "run",
        "clean",
        "doctor",
        "toolchains",
        "init",
    ] {
        assert!(text.contains(command), "help must mention {command}");
    }
}

// ------------------------------------------------------------ detect ----

#[test]
fn detect_json_reports_cargo_project() {
    let guard = make_project(&[("Cargo.toml", "[package]\nname=\"x\"\nversion=\"0.1.0\"\n")]);
    let output = run_cli(&["detect", "--json"], &guard.project_dir(), None, &[]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = stdout_json(&output);
    assert_eq!(json["schema_version"], 1);
    assert_eq!(json["command"], "detect");
    assert_eq!(json["success"], true);
    assert_eq!(json["exit_code"], 0);
    assert_eq!(json["detect"]["primary"]["kind"], "rust");
    assert_eq!(json["detect"]["primary"]["confidence"], "high");
    assert_eq!(json["detect"]["primary"]["markers"][0], "Cargo.toml");
}

#[test]
fn detect_on_unknown_project_exits_3_with_error_info() {
    let guard = make_project(&[("readme.md", "nothing")]);
    let output = run_cli(&["detect", "--json"], &guard.project_dir(), None, &[]);
    assert_eq!(output.status.code(), Some(3));
    let json = stdout_json(&output);
    assert_eq!(json["success"], false);
    assert_eq!(json["error"]["code"], "not_detected");
    assert!(
        json["error"]["hint"]
            .as_str()
            .unwrap()
            .contains("Cargo.toml")
    );
}

#[test]
fn detect_missing_directory_is_a_usage_error() {
    let tmp = TempDir::new().unwrap();
    let output = run_cli(
        &["detect", "does-not-exist", "--json"],
        tmp.path(),
        None,
        &[],
    );
    assert_eq!(output.status.code(), Some(2));
    let json = stdout_json(&output);
    // A missing path is a PATH error — never reported as a detection result.
    assert_eq!(json["error"]["code"], "invalid_path");
    let message = json["error"]["message"].as_str().unwrap();
    assert!(message.contains("does not exist"), "{message}");
    assert!(!message.contains("no recognizable project"), "{message}");
}

#[test]
fn detect_ambiguous_project_requires_explicit_choice() {
    let guard = make_project(&[("pom.xml", ""), ("build.gradle", "")]);
    let output = run_cli(&["detect", "--json"], &guard.project_dir(), None, &[]);
    // `detect` itself succeeds (it detected candidates) but reports ambiguity.
    assert_eq!(output.status.code(), Some(5));
    let json = stdout_json(&output);
    assert_eq!(json["detect"]["ambiguous"], true);
    assert_eq!(json["detect"]["candidates"].as_array().unwrap().len(), 2);
}

// -------------------------------------------------------------- build ----

#[test]
fn build_invokes_the_detected_build_system() {
    let guard = make_project(&[("Cargo.toml", "[package]\nname=\"x\"\n")]);
    let fake = TempDir::new().unwrap();
    let cargo = testkit::FakeTool::new("cargo");
    // Move the fake tool into our isolated PATH dir.
    fs::copy(
        cargo.program(),
        fake.path()
            .join(if cfg!(windows) { "cargo.cmd" } else { "cargo" }),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(fake.path().join("cargo"), fs::Permissions::from_mode(0o755)).unwrap();
    }
    let (log_key, log_value) = cargo.log_env();
    let output = run_cli(
        &["build", "--json"],
        &guard.project_dir(),
        Some(fake.path()),
        &[(log_key.as_str(), log_value.as_str())],
    );
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = stdout_json(&output);
    assert_eq!(json["success"], true);
    assert_eq!(json["build"]["kind"], "rust");
    let command = json["build"]["command"].as_array().unwrap();
    assert!(command[0].as_str().unwrap().ends_with(if cfg!(windows) {
        "cargo.cmd"
    } else {
        "cargo"
    }));
    assert_eq!(command[1], "build");
    // The version probe and the real invocation both run; the last line is
    // the actual build command.
    let lines = cargo.log_lines();
    assert!(
        lines.iter().any(|line| line.contains("--version")),
        "lines: {lines:?}"
    );
    assert_eq!(lines.last().map(String::as_str), Some("fake cargo build"));
}

#[test]
fn build_with_failing_tool_exits_1_and_preserves_tool_exit_code() {
    let guard = make_project(&[("Cargo.toml", "")]);
    let fake = TempDir::new().unwrap();
    let cargo = testkit::FakeTool::new("cargo");
    fs::copy(
        cargo.program(),
        fake.path()
            .join(if cfg!(windows) { "cargo.cmd" } else { "cargo" }),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(fake.path().join("cargo"), fs::Permissions::from_mode(0o755)).unwrap();
    }
    let output = run_cli(
        &["build", "--json"],
        &guard.project_dir(),
        Some(fake.path()),
        &[("FAKE_TOOL_EXIT", "42")],
    );
    assert_eq!(output.status.code(), Some(1));
    let json = stdout_json(&output);
    assert_eq!(json["success"], false);
    assert_eq!(json["exit_code"], 1);
    assert_eq!(json["build"]["exit_code"], 42);
    assert_eq!(json["error"]["code"], "tool_failed");
}

#[test]
fn build_without_required_tool_exits_4() {
    let guard = make_project(&[("Cargo.toml", "")]);
    // A truly empty PATH: no tool resolution possible at all.
    let output = {
        let mut command = Command::new(binary());
        command
            .args(["build", "--json"])
            .current_dir(guard.project_dir())
            .env("PATH", "");
        command.output().unwrap()
    };
    assert_eq!(output.status.code(), Some(4));
    let json = stdout_json(&output);
    assert_eq!(json["error"]["code"], "tool_missing");
    assert!(json["error"]["hint"].as_str().unwrap().contains("rustup"));
}

#[test]
fn ambiguous_build_exits_5_and_kind_flag_resolves_it() {
    let guard = make_project(&[("pom.xml", ""), ("build.gradle", "")]);
    let output = run_cli(&["build", "--json"], &guard.project_dir(), None, &[]);
    assert_eq!(output.status.code(), Some(5));
    let json = stdout_json(&output);
    assert_eq!(json["error"]["code"], "ambiguous_project");

    // With --kind the ambiguity is resolved; the operation then proceeds and
    // fails on the missing tool (exit 4), which names the resolved tool.
    let output = run_cli(
        &["build", "--json", "--kind", "gradle"],
        &guard.project_dir(),
        None,
        &[],
    );
    assert_eq!(output.status.code(), Some(4));
    let json = stdout_json(&output);
    assert_eq!(json["error"]["code"], "tool_missing");
    assert!(
        json["error"]["message"]
            .as_str()
            .unwrap()
            .contains("gradle")
    );
}

#[test]
fn run_json_mode_captures_output() {
    let guard = make_project(&[("pyproject.toml", ""), ("main.py", "print('hi')")]);
    let fake = TempDir::new().unwrap();
    let python = testkit::FakeTool::new(if cfg!(windows) { "python" } else { "python3" });
    let exe_name = if cfg!(windows) {
        "python.cmd"
    } else {
        "python3"
    };
    fs::copy(python.program(), fake.path().join(exe_name)).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(
            fake.path().join(exe_name),
            fs::Permissions::from_mode(0o755),
        )
        .unwrap();
    }
    let output = run_cli(
        &["run", "--json"],
        &guard.project_dir(),
        Some(fake.path()),
        &[],
    );
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = stdout_json(&output);
    assert_eq!(json["success"], true);
    assert!(
        json["run"]["stdout"]
            .as_str()
            .unwrap()
            .contains("fake python3 ok")
    );
}

// ------------------------------------------------------ discovery boundaries ----

#[test]
fn detect_in_markerless_subdir_does_not_report_ancestor_project() {
    // Regression for the 0.1.0 report: `detect unknown-project` inside a
    // repository used to silently report the ancestor's Cargo project.
    let (tmp, dir) = make_project_with_name("outer-proj", &[("Cargo.toml", "")]);
    let unknown = dir.join("unknown-project");
    fs::create_dir_all(&unknown).unwrap();
    fs::write(unknown.join("readme.md"), "nothing here").unwrap();

    // Default: not detected (exit 3), never an ancestor project.
    let output = run_cli(&["detect", "--json"], &unknown, None, &[]);
    assert_eq!(output.status.code(), Some(3));
    let json = stdout_json(&output);
    assert_eq!(json["error"]["code"], "not_detected");
    assert_eq!(json["detect"]["searched_upwards"], false);
    assert!(json["detect"]["root"].is_null());

    // Opt-in flag discovers the ancestor and reports that it did.
    let output = run_cli(
        &["detect", "--json", "--search-parents"],
        &unknown,
        None,
        &[],
    );
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = stdout_json(&output);
    assert_eq!(json["detect"]["primary"]["kind"], "rust");
    assert_eq!(
        json["detect"]["root"],
        raven_core::path_to_string(&dir.canonicalize().unwrap())
    );
    assert_eq!(json["detect"]["searched_upwards"], true);
    drop(tmp);
}

#[test]
fn build_in_markerless_subdir_also_respects_the_boundary() {
    let (tmp, dir) = make_project_with_name("outer-proj", &[("Cargo.toml", "")]);
    let nested = dir.join("docs/notes");
    fs::create_dir_all(&nested).unwrap();

    let output = run_cli(&["build", "--json"], &nested, None, &[]);
    assert_eq!(output.status.code(), Some(3));
    let json = stdout_json(&output);
    assert_eq!(json["error"]["code"], "not_detected");

    // --search-parents is global: operations honour it too. With an empty
    // PATH detection succeeds but the tool is missing (exit 4, not 3) —
    // deterministic on machines that do or do not have real toolchains.
    let mut command = Command::new(binary());
    command
        .args(["build", "--json", "--search-parents"])
        .current_dir(&nested)
        .env("PATH", "");
    let output = command.output().unwrap();
    assert_eq!(output.status.code(), Some(4));
    let json = stdout_json(&output);
    assert_eq!(json["error"]["code"], "tool_missing");
    drop(tmp);
}

// ------------------------------------------------------------------ cmake ----

#[test]
fn cmake_project_is_detected_without_cmake_installed() {
    // Detection never requires a toolchain — even with an empty PATH.
    let guard = make_project(&[("CMakeLists.txt", "cmake_minimum_required(VERSION 3.16)\n")]);
    let mut command = Command::new(binary());
    command
        .args(["detect", "--json"])
        .current_dir(guard.project_dir())
        .env("PATH", "");
    let output = command.output().unwrap();
    assert!(output.status.success());
    let json = stdout_json(&output);
    assert_eq!(json["detect"]["primary"]["kind"], "cmake");
}

#[test]
fn cmake_build_without_cmake_explains_detected_project_and_missing_tool() {
    let guard = make_project(&[("CMakeLists.txt", "cmake_minimum_required(VERSION 3.16)\n")]);
    let mut command = Command::new(binary());
    command
        .args(["build", "--json"])
        .current_dir(guard.project_dir())
        .env("PATH", "");
    let output = command.output().unwrap();
    assert_eq!(output.status.code(), Some(4));
    let json = stdout_json(&output);
    assert_eq!(json["error"]["code"], "tool_missing");
    assert!(
        json["error"]["message"].as_str().unwrap().contains("cmake"),
        "{}",
        json["error"]["message"]
    );
    let hint = json["error"]["hint"].as_str().unwrap();
    assert!(
        hint.contains("detected"),
        "hint must mention the successful detection: {hint}"
    );
    assert!(
        hint.contains("cmake.org"),
        "hint must be actionable: {hint}"
    );
}

#[test]
fn cmake_run_without_run_config_is_unsupported_with_guidance() {
    // `run` never resolves tools for CMake: it refuses first, with guidance.
    let guard = make_project(&[("CMakeLists.txt", "cmake_minimum_required(VERSION 3.16)\n")]);
    let mut command = Command::new(binary());
    command
        .args(["run", "--json"])
        .current_dir(guard.project_dir())
        .env("PATH", "");
    let output = command.output().unwrap();
    assert_eq!(output.status.code(), Some(9));
    let json = stdout_json(&output);
    assert_eq!(json["error"]["code"], "unsupported_operation");
    let message = json["error"]["message"].as_str().unwrap();
    assert!(message.contains("run"), "{message}");
    assert!(
        json["error"]["hint"]
            .as_str()
            .unwrap()
            .contains("raven-pico.toml"),
        "hint must point at the configuration"
    );
}

#[test]
fn cmake_run_with_configured_command_executes_it() {
    let guard = make_project(&[
        ("CMakeLists.txt", "cmake_minimum_required(VERSION 3.16)\n"),
        (
            "raven-pico.toml",
            "schema_version = 1\n[run]\ncommand = \"hello\"\n",
        ),
    ]);
    let fake = TempDir::new().unwrap();
    let hello = testkit::FakeTool::new("hello");
    fs::copy(
        hello.program(),
        fake.path()
            .join(if cfg!(windows) { "hello.cmd" } else { "hello" }),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(fake.path().join("hello"), fs::Permissions::from_mode(0o755)).unwrap();
    }
    let log_env = ("FAKE_TOOL_LOG", hello.log.display().to_string());
    let output = run_cli(
        &["run", "--json"],
        &guard.project_dir(),
        Some(fake.path()),
        &[("FAKE_TOOL_LOG", log_env.1.as_str())],
    );
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = stdout_json(&output);
    assert_eq!(json["success"], true);
    assert!(hello.log_lines().iter().any(|line| line == "fake hello"));
}

// ---------------------------------------------------------------- init ----

#[test]
fn init_creates_config_and_refuses_to_overwrite() {
    let guard = make_project(&[("Cargo.toml", "")]);
    let dir = guard.project_dir();

    let output = run_cli(&["init", "--json"], &dir, None, &[]);
    assert!(output.status.success());
    let json = stdout_json(&output);
    assert_eq!(json["init"]["created"], true);
    assert_eq!(json["init"]["kind"], "rust");
    let created = fs::read_to_string(dir.join("raven-pico.toml")).unwrap();
    assert!(created.contains("schema_version = 1"));
    assert!(created.contains("kind = \"rust\""));

    // Second run refuses (exit 10) and does NOT touch the file.
    let before = fs::read_to_string(dir.join("raven-pico.toml")).unwrap();
    let output = run_cli(&["init", "--json"], &dir, None, &[]);
    assert_eq!(output.status.code(), Some(10));
    let json = stdout_json(&output);
    assert_eq!(json["error"]["code"], "config_exists");
    let after = fs::read_to_string(dir.join("raven-pico.toml")).unwrap();
    assert_eq!(before, after, "existing config must not be modified");

    // --force is the explicit permission to overwrite.
    let output = run_cli(&["init", "--force", "--json"], &dir, None, &[]);
    assert!(output.status.success());
}

#[test]
fn init_preserves_unrelated_files() {
    let guard = make_project(&[("notes.txt", "important")]);
    let dir = guard.project_dir();
    let output = run_cli(&["init"], &dir, None, &[]);
    assert!(output.status.success());
    assert_eq!(
        fs::read_to_string(dir.join("notes.txt")).unwrap(),
        "important"
    );
    // Only the config file was created.
    let entries: Vec<_> = fs::read_dir(&dir).unwrap().collect();
    assert_eq!(entries.len(), 2);
}

// ------------------------------------------------------- configuration ----

#[test]
fn malformed_config_blocks_operations_with_exit_6() {
    let guard = make_project(&[("Cargo.toml", ""), ("raven-pico.toml", "not [valid toml")]);
    let output = run_cli(&["build", "--json"], &guard.project_dir(), None, &[]);
    assert_eq!(output.status.code(), Some(6));
    let json = stdout_json(&output);
    assert_eq!(json["error"]["code"], "config_invalid");
}

#[test]
fn config_defined_build_command_is_executed() {
    let guard = make_project(&[(
        "raven-pico.toml",
        "schema_version = 1\n[build]\ncommand = \"make\"\n",
    )]);
    let fake = TempDir::new().unwrap();
    let make = testkit::FakeTool::new("make");
    fs::copy(
        make.program(),
        fake.path()
            .join(if cfg!(windows) { "make.cmd" } else { "make" }),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(fake.path().join("make"), fs::Permissions::from_mode(0o755)).unwrap();
    }
    let (log_key, log_value) = make.log_env();
    let output = run_cli(
        &["build", "--json"],
        &guard.project_dir(),
        Some(fake.path()),
        &[(log_key.as_str(), log_value.as_str())],
    );
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = stdout_json(&output);
    assert_eq!(json["build"]["kind"], "custom");
    assert_eq!(make.log_lines(), vec!["fake make"]);
}

// ---------------------------------------------------- doctor/toolchains ----

#[test]
fn doctor_json_reports_checks_and_exits_0() {
    let tmp = TempDir::new().unwrap();
    let output = run_cli(&["doctor", "--json"], tmp.path(), None, &[]);
    assert!(output.status.success());
    let json = stdout_json(&output);
    assert_eq!(json["command"], "doctor");
    let checks = json["doctor"]["checks"].as_array().unwrap();
    assert!(!checks.is_empty());
    assert!(json["doctor"]["os"].is_string());
    assert!(json["doctor"]["arch"].is_string());
}

#[test]
fn toolchains_json_lists_known_tools() {
    let tmp = TempDir::new().unwrap();
    let output = run_cli(&["toolchains", "--json"], tmp.path(), None, &[]);
    assert!(output.status.success());
    let json = stdout_json(&output);
    let checks = json["toolchains"]["checks"].as_array().unwrap();
    let ids: Vec<&str> = checks
        .iter()
        .map(|check| check["id"].as_str().unwrap())
        .collect();
    for expected in [
        "cargo", "cmake", "dotnet", "python", "godot", "mvn", "gradle", "lua",
    ] {
        assert!(ids.contains(&expected), "toolchains must list {expected}");
    }
}

// ------------------------------------------------------ paths & spaces ----

#[test]
fn paths_with_spaces_work_end_to_end() {
    let (tmp, dir) = make_project_with_name("my project (copy)", &[("Cargo.toml", "")]);
    let output = run_cli(&["detect", "--json"], &dir, None, &[]);
    assert!(output.status.success());
    let json = stdout_json(&output);
    // JSON paths use the shared serialization (Windows verbatim prefixes
    // removed when safe), so compare against that exact form.
    assert_eq!(
        json["detect"]["root"],
        raven_core::path_to_string(&dir.canonicalize().unwrap())
    );
    drop(tmp);
}

#[test]
fn parent_search_finds_project_root_from_a_nested_directory() {
    let (tmp, dir) = make_project_with_name("outer-proj", &[("Cargo.toml", "")]);
    let nested = dir.join("src/commands");
    fs::create_dir_all(&nested).unwrap();
    // Parent discovery is opt-in since 0.2.0: --search-parents enables it.
    let output = run_cli(
        &["detect", "--json", "--search-parents"],
        &nested,
        None,
        &[],
    );
    assert!(output.status.success());
    let json = stdout_json(&output);
    assert_eq!(json["detect"]["primary"]["kind"], "rust");
    assert_eq!(
        json["detect"]["root"],
        raven_core::path_to_string(&dir.canonicalize().unwrap())
    );
    assert_eq!(json["detect"]["searched_upwards"], true);
    drop(tmp);
}
