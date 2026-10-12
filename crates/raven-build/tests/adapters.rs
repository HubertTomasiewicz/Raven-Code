//! Adapter tests driven by fake toolchains: command construction, config
//! precedence, unsupported operations and tool-missing errors — without any
//! real compiler installed.

use raven_build::proc::path_dirs;
use raven_build::{
    CancelToken, Op, OpContext, OperationOutcome, Profile, StdioMode, execute_operation,
};
use raven_core::config::PicoConfig;
use raven_core::error::PicoError;
use raven_core::kind::ProjectKind;
use raven_core::progress::SilentSink;
use tempfile::TempDir;
use testkit::{FakeTool, marker_project};

fn ctx<'a>(
    root: &std::path::Path,
    kind: ProjectKind,
    config: Option<&'a PicoConfig>,
    search_paths: Vec<std::path::PathBuf>,
    env: std::collections::BTreeMap<String, String>,
) -> OpContext<'a> {
    OpContext {
        root: root.to_path_buf(),
        kind,
        config,
        profile: Profile::Debug,
        extra_args: Vec::new(),
        env,
        search_paths,
        sink: Some(&SilentSink),
        cancel: CancelToken::new(),
        stdio: StdioMode::Capture,
    }
}

fn expect_success(outcome: &OperationOutcome) {
    assert!(outcome.success(), "expected success, got: {outcome:?}");
}

// ------------------------------------------------------------------ cargo ----

#[test]
fn cargo_build_constructs_expected_command() {
    let project = TempDir::new().unwrap();
    marker_project(project.path(), &[("Cargo.toml", "[package]\nname=\"x\"\n")]).unwrap();
    let tool = FakeTool::new("cargo");
    let (log_key, log_value) = tool.log_env();

    let context = ctx(
        project.path(),
        ProjectKind::Rust,
        None,
        tool.search_paths(),
        [(log_key, log_value)].into_iter().collect(),
    );
    let outcome = execute_operation(&context, Op::Build).unwrap();
    expect_success(&outcome);
    assert_eq!(tool.log_lines(), vec!["fake cargo build"]);
    // Artifacts: the well-known target dir for the profile is reported.
    assert_eq!(outcome.artifacts.len(), 1);
    assert!(outcome.tool.as_ref().unwrap().path.is_some());
}

#[test]
fn cargo_release_flag_and_tool_args_are_forwarded() {
    let project = TempDir::new().unwrap();
    marker_project(project.path(), &[("Cargo.toml", "")]).unwrap();
    let tool = FakeTool::new("cargo");
    let (log_key, log_value) = tool.log_env();

    let mut context = ctx(
        project.path(),
        ProjectKind::Rust,
        None,
        tool.search_paths(),
        [(log_key, log_value)].into_iter().collect(),
    );
    context.profile = Profile::Release;
    context.extra_args = vec!["--locked".to_string()];
    let outcome = execute_operation(&context, Op::Test).unwrap();
    expect_success(&outcome);
    assert_eq!(tool.log_lines(), vec!["fake cargo test --release --locked"]);
}

#[test]
fn missing_cargo_is_tool_missing_with_hint() {
    let project = TempDir::new().unwrap();
    marker_project(project.path(), &[("Cargo.toml", "")]).unwrap();
    let empty = TempDir::new().unwrap();
    let context = ctx(
        project.path(),
        ProjectKind::Rust,
        None,
        vec![empty.path().to_path_buf()],
        Default::default(),
    );
    let error = execute_operation(&context, Op::Build).unwrap_err();
    let exit_code = error.exit_code();
    match &error {
        PicoError::ToolMissing { tool, hint } => {
            assert_eq!(tool, "cargo");
            assert!(hint.contains("rustup"));
        }
        other => panic!("expected ToolMissing, got {other:?}"),
    }
    assert_eq!(exit_code, raven_core::error::exit_codes::TOOL_MISSING);
}

#[test]
fn cargo_clean_has_no_release_flag() {
    let project = TempDir::new().unwrap();
    marker_project(project.path(), &[("Cargo.toml", "")]).unwrap();
    let tool = FakeTool::new("cargo");
    let (log_key, log_value) = tool.log_env();
    let mut context = ctx(
        project.path(),
        ProjectKind::Rust,
        None,
        tool.search_paths(),
        [(log_key, log_value)].into_iter().collect(),
    );
    context.profile = Profile::Release;
    let outcome = execute_operation(&context, Op::Clean).unwrap();
    expect_success(&outcome);
    assert_eq!(tool.log_lines(), vec!["fake cargo clean"]);
}

// ------------------------------------------------------------------ cmake ----

#[test]
fn cmake_configures_before_building() {
    let project = TempDir::new().unwrap();
    marker_project(project.path(), &[("CMakeLists.txt", "")]).unwrap();
    let tool = FakeTool::new("cmake");
    let (log_key, log_value) = tool.log_env();
    let context = ctx(
        project.path(),
        ProjectKind::CMake,
        None,
        tool.search_paths(),
        [(log_key, log_value)].into_iter().collect(),
    );
    let outcome = execute_operation(&context, Op::Build).unwrap();
    expect_success(&outcome);
    let lines = tool.log_lines();
    assert_eq!(lines.len(), 2, "configure then build: {lines:?}");
    assert!(lines[0].contains("-S") && lines[0].contains("-B"));
    assert!(lines[1].starts_with("fake cmake --build"));
    assert!(
        outcome
            .note
            .as_deref()
            .unwrap()
            .contains("configured build directory")
    );
}

#[test]
fn cmake_clean_refuses_to_delete_a_non_cmake_directory() {
    let project = TempDir::new().unwrap();
    marker_project(project.path(), &[("CMakeLists.txt", "")]).unwrap();
    // A "build" directory that is NOT a CMake build tree.
    std::fs::create_dir_all(project.path().join("build")).unwrap();
    std::fs::write(project.path().join("build/precious.txt"), "user data").unwrap();

    let tool = FakeTool::new("cmake");
    let context = ctx(
        project.path(),
        ProjectKind::CMake,
        None,
        tool.search_paths(),
        Default::default(),
    );
    let error = execute_operation(&context, Op::Clean).unwrap_err();
    assert!(error.to_string().contains("refusing to remove"), "{error}");
    // The user's directory is untouched.
    assert!(project.path().join("build/precious.txt").is_file());
}

#[test]
fn cmake_run_requires_configured_command() {
    let project = TempDir::new().unwrap();
    marker_project(project.path(), &[("CMakeLists.txt", "")]).unwrap();
    let tool = FakeTool::new("cmake");
    let context = ctx(
        project.path(),
        ProjectKind::CMake,
        None,
        tool.search_paths(),
        Default::default(),
    );
    let error = execute_operation(&context, Op::Run).unwrap_err();
    assert!(matches!(error, PicoError::UnsupportedOperation { .. }));
    assert_eq!(
        error.exit_code(),
        raven_core::error::exit_codes::UNSUPPORTED
    );
}

#[test]
fn cmake_run_does_not_invent_an_executable_path() {
    // Without a [run] configuration Pico must refuse BEFORE spawning
    // anything and must not fabricate a binary path.
    let project = TempDir::new().unwrap();
    marker_project(project.path(), &[("CMakeLists.txt", "")]).unwrap();
    let tool = FakeTool::new("cmake");
    let (log_key, log_value) = tool.log_env();
    let context = ctx(
        project.path(),
        ProjectKind::CMake,
        None,
        tool.search_paths(),
        [(log_key, log_value)].into_iter().collect(),
    );
    let error = execute_operation(&context, Op::Run).unwrap_err();
    assert!(error.to_string().contains("run"), "{error}");
    assert!(error.hint().unwrap().contains("raven-pico.toml"));
    assert!(
        tool.log_lines().is_empty(),
        "no tool may be spawned for run"
    );
}

#[test]
fn cmake_release_forwards_build_type_and_config() {
    let project = TempDir::new().unwrap();
    marker_project(project.path(), &[("CMakeLists.txt", "")]).unwrap();
    let tool = FakeTool::new("cmake");
    let (log_key, log_value) = tool.log_env();
    let mut context = ctx(
        project.path(),
        ProjectKind::CMake,
        None,
        tool.search_paths(),
        [(log_key, log_value)].into_iter().collect(),
    );
    context.profile = Profile::Release;
    let outcome = execute_operation(&context, Op::Build).unwrap();
    expect_success(&outcome);
    let lines = tool.log_lines();
    assert_eq!(lines.len(), 2);
    assert!(
        lines[0].contains("-DCMAKE_BUILD_TYPE=Release"),
        "{:?}",
        lines
    );
    assert!(
        lines[1].contains("--config") && lines[1].contains("Release"),
        "{:?}",
        lines
    );
}

#[test]
fn cmake_build_dir_comes_from_the_config_section() {
    let project = TempDir::new().unwrap();
    marker_project(project.path(), &[("CMakeLists.txt", "")]).unwrap();
    let config_text = "schema_version = 1\n[cmake]\nbuild_dir = \"out/clang-build\"\n";
    let config = PicoConfig::parse(config_text).unwrap();
    let tool = FakeTool::new("cmake");
    let (log_key, log_value) = tool.log_env();
    let context = ctx(
        project.path(),
        ProjectKind::CMake,
        Some(&config),
        tool.search_paths(),
        [(log_key, log_value)].into_iter().collect(),
    );
    let outcome = execute_operation(&context, Op::Build).unwrap();
    expect_success(&outcome);
    let lines = tool.log_lines();
    assert!(lines[0].contains("out/clang-build"), "{:?}", lines);
    assert!(lines[1].contains("out/clang-build"), "{:?}", lines);
}

#[test]
fn cmake_test_runs_ctest_against_the_build_dir() {
    let project = TempDir::new().unwrap();
    marker_project(project.path(), &[("CMakeLists.txt", "")]).unwrap();
    // A configured tree: CTest runs directly, no configure step.
    std::fs::create_dir_all(project.path().join("build")).unwrap();
    std::fs::write(project.path().join("build/CMakeCache.txt"), "").unwrap();

    let cmake = FakeTool::new("cmake");
    let ctest = FakeTool::new("ctest");
    // Both fakes append to the SAME log (shared FAKE_TOOL_LOG env).
    let (log_key, log_value) = cmake.log_env();
    let mut search_paths = cmake.search_paths();
    search_paths.extend(ctest.search_paths());
    let context = ctx(
        project.path(),
        ProjectKind::CMake,
        None,
        search_paths,
        [(log_key, log_value)].into_iter().collect(),
    );
    let outcome = execute_operation(&context, Op::Test).unwrap();
    expect_success(&outcome);
    let lines = cmake.log_lines();
    assert_eq!(lines.len(), 1, "configured tree: ctest only: {lines:?}");
    let ctest_line = &lines[0];
    assert!(ctest_line.starts_with("fake ctest"), "{ctest_line}");
    assert!(ctest_line.contains("--test-dir"));
    assert!(ctest_line.contains("--output-on-failure"));
    assert!(ctest_line.contains("build"));
}

#[test]
fn cmake_clean_uses_the_native_clean_target_when_configured() {
    let project = TempDir::new().unwrap();
    marker_project(project.path(), &[("CMakeLists.txt", "")]).unwrap();
    std::fs::create_dir_all(project.path().join("build")).unwrap();
    std::fs::write(project.path().join("build/CMakeCache.txt"), "").unwrap();

    let tool = FakeTool::new("cmake");
    let (log_key, log_value) = tool.log_env();
    let context = ctx(
        project.path(),
        ProjectKind::CMake,
        None,
        tool.search_paths(),
        [(log_key, log_value)].into_iter().collect(),
    );
    let outcome = execute_operation(&context, Op::Clean).unwrap();
    expect_success(&outcome);
    let lines = tool.log_lines();
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(
        lines[0].contains("--target") && lines[0].contains("clean"),
        "{:?}",
        lines
    );
    // The configured build tree still exists (native clean ran, not rm).
    assert!(project.path().join("build/CMakeCache.txt").is_file());
}

#[test]
fn cmake_missing_tool_hint_explains_detected_project_and_next_steps() {
    let project = TempDir::new().unwrap();
    marker_project(project.path(), &[("CMakeLists.txt", "")]).unwrap();
    let empty = TempDir::new().unwrap();
    let context = ctx(
        project.path(),
        ProjectKind::CMake,
        None,
        vec![empty.path().to_path_buf()],
        Default::default(),
    );
    let error = execute_operation(&context, Op::Build).unwrap_err();
    match &error {
        PicoError::ToolMissing { tool, hint } => {
            assert_eq!(tool, "cmake");
            assert!(hint.contains("detected"), "hint: {hint}");
            assert!(hint.contains("cmake.org"), "hint: {hint}");
        }
        other => panic!("expected ToolMissing, got {other:?}"),
    }
    assert_eq!(
        error.exit_code(),
        raven_core::error::exit_codes::TOOL_MISSING
    );
}

// ----------------------------------------------------------------- dotnet ----

#[test]
fn dotnet_build_passes_configuration() {
    let project = TempDir::new().unwrap();
    marker_project(project.path(), &[("App.csproj", "<Project/>")]).unwrap();
    let tool = FakeTool::new("dotnet");
    let (log_key, log_value) = tool.log_env();
    let mut context = ctx(
        project.path(),
        ProjectKind::DotNet,
        None,
        tool.search_paths(),
        [(log_key, log_value)].into_iter().collect(),
    );
    context.profile = Profile::Release;
    let outcome = execute_operation(&context, Op::Build).unwrap();
    expect_success(&outcome);
    assert_eq!(tool.log_lines(), vec!["fake dotnet build -c Release"]);
}

// ----------------------------------------------------------------- python ----

#[test]
fn python_run_uses_main_py() {
    let project = TempDir::new().unwrap();
    marker_project(
        project.path(),
        &[("pyproject.toml", ""), ("main.py", "print('hi')")],
    )
    .unwrap();
    let tool = FakeTool::new("python3");
    let (log_key, log_value) = tool.log_env();
    let context = ctx(
        project.path(),
        ProjectKind::Python,
        None,
        tool.search_paths(),
        [(log_key, log_value)].into_iter().collect(),
    );
    let outcome = execute_operation(&context, Op::Run).unwrap();
    expect_success(&outcome);
    let logged = tool.log_lines().join(" ");
    assert!(logged.contains("main.py"), "{logged}");
}

#[test]
fn python_build_is_not_supported() {
    let project = TempDir::new().unwrap();
    marker_project(project.path(), &[("pyproject.toml", "")]).unwrap();
    let tool = FakeTool::new("python3");
    let context = ctx(
        project.path(),
        ProjectKind::Python,
        None,
        tool.search_paths(),
        Default::default(),
    );
    let error = execute_operation(&context, Op::Build).unwrap_err();
    assert!(matches!(error, PicoError::UnsupportedOperation { .. }));
}

// ------------------------------------------------------------------ godot ----

#[test]
fn godot_run_targets_the_project_path() {
    let project = TempDir::new().unwrap();
    marker_project(project.path(), &[("project.godot", "config_version=5")]).unwrap();
    let tool = FakeTool::new("godot");
    let (log_key, log_value) = tool.log_env();
    let context = ctx(
        project.path(),
        ProjectKind::Godot,
        None,
        tool.search_paths(),
        [(log_key, log_value)].into_iter().collect(),
    );
    let outcome = execute_operation(&context, Op::Run).unwrap();
    expect_success(&outcome);
    let logged = tool.log_lines().join(" ");
    assert!(logged.contains("--path"), "{logged}");
    assert!(logged.contains(project.path().display().to_string().as_str()));
}

#[test]
fn godot_clean_removes_only_the_cache_directory() {
    let project = TempDir::new().unwrap();
    marker_project(project.path(), &[("project.godot", "")]).unwrap();
    std::fs::create_dir_all(project.path().join(".godot")).unwrap();
    std::fs::write(project.path().join("scene.tscn"), "user file").unwrap();

    let context = ctx(
        project.path(),
        ProjectKind::Godot,
        None,
        vec![],
        Default::default(),
    );
    let outcome = execute_operation(&context, Op::Clean).unwrap();
    expect_success(&outcome);
    assert!(!project.path().join(".godot").exists());
    assert!(project.path().join("scene.tscn").is_file());
}

// ------------------------------------------------------------- maven/gradle ----

#[test]
fn maven_test_runs_the_test_goal() {
    let project = TempDir::new().unwrap();
    marker_project(project.path(), &[("pom.xml", "<project/>")]).unwrap();
    let tool = FakeTool::new("mvn");
    let (log_key, log_value) = tool.log_env();
    let context = ctx(
        project.path(),
        ProjectKind::Maven,
        None,
        tool.search_paths(),
        [(log_key, log_value)].into_iter().collect(),
    );
    let outcome = execute_operation(&context, Op::Test).unwrap();
    expect_success(&outcome);
    assert_eq!(tool.log_lines(), vec!["fake mvn test"]);
}

#[test]
fn gradle_prefers_the_wrapper() {
    let project = TempDir::new().unwrap();
    marker_project(project.path(), &[("build.gradle", "")]).unwrap();
    let wrapper = FakeTool::new("gradlew");
    // Copy the fake wrapper into the project root so the adapter finds it.
    std::fs::copy(wrapper.program(), project.path().join("gradlew")).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            project.path().join("gradlew"),
            std::fs::Permissions::from_mode(0o755),
        )
        .unwrap();
    }
    let (log_key, log_value) = wrapper.log_env();
    let mut env = std::collections::BTreeMap::new();
    env.insert(log_key, log_value);
    let context = ctx(project.path(), ProjectKind::Gradle, None, vec![], env);
    let outcome = execute_operation(&context, Op::Build).unwrap();
    expect_success(&outcome);
    assert_eq!(wrapper.log_lines(), vec!["fake gradlew assemble"]);
}

#[test]
fn gradle_without_wrapper_or_global_is_tool_missing() {
    let project = TempDir::new().unwrap();
    marker_project(project.path(), &[("build.gradle", "")]).unwrap();
    let empty = TempDir::new().unwrap();
    let context = ctx(
        project.path(),
        ProjectKind::Gradle,
        None,
        vec![empty.path().to_path_buf()],
        Default::default(),
    );
    let error = execute_operation(&context, Op::Build).unwrap_err();
    match &error {
        PicoError::ToolMissing { tool, hint } => {
            assert_eq!(tool, "gradle");
            assert!(hint.contains("wrapper"), "{hint}");
        }
        other => panic!("expected ToolMissing, got {other:?}"),
    }
    assert_eq!(
        error.exit_code(),
        raven_core::error::exit_codes::TOOL_MISSING
    );
}

// -------------------------------------------------------------------- lua ----

#[test]
fn lua_run_executes_main_lua() {
    let project = TempDir::new().unwrap();
    marker_project(project.path(), &[("main.lua", "print('hi')")]).unwrap();
    let tool = FakeTool::new("lua");
    let (log_key, log_value) = tool.log_env();
    let context = ctx(
        project.path(),
        ProjectKind::Lua,
        None,
        tool.search_paths(),
        [(log_key, log_value)].into_iter().collect(),
    );
    let outcome = execute_operation(&context, Op::Run).unwrap();
    expect_success(&outcome);
    let logged = tool.log_lines().join(" ");
    assert!(logged.contains("main.lua"), "{logged}");
}

// ----------------------------------------------------------------- custom ----

#[test]
fn config_commands_execute_and_precede_native_adapters() {
    let project = TempDir::new().unwrap();
    // A cargo project whose build is overridden by a config command.
    marker_project(project.path(), &[("Cargo.toml", "")]).unwrap();
    let marker = project.path().join("built.marker");
    let script_body = if cfg!(windows) {
        format!("type nul > \"{}\"", marker.display())
    } else {
        format!("touch \"{}\"", marker.display())
    };
    let make = FakeTool::with_body("make", &script_body);
    let config_text = "schema_version = 1\n[build]\ncommand = \"make\"\n";
    let config = PicoConfig::parse(config_text).unwrap();
    let (log_key, log_value) = make.log_env();
    let mut env = std::collections::BTreeMap::new();
    env.insert(log_key, log_value);
    let context = ctx(
        project.path(),
        ProjectKind::Rust,
        Some(&config),
        make.search_paths(),
        env,
    );
    let outcome = execute_operation(&context, Op::Build).unwrap();
    expect_success(&outcome);
    // The configured command REALLY ran (a marker file was created).
    assert!(marker.is_file(), "configured command must execute");
    assert_eq!(outcome.command.as_deref(), Some(&["make".to_string()][..]));
}

#[test]
fn failing_config_command_preserves_exit_code() {
    let project = TempDir::new().unwrap();
    marker_project(
        project.path(),
        &[(
            "raven-pico.toml",
            "schema_version = 1\n[build]\ncommand = \"failtool\"\n",
        )],
    )
    .unwrap();
    let tool = FakeTool::new("failtool");
    let (log_key, log_value) = tool.log_env();
    let mut env = std::collections::BTreeMap::new();
    env.insert(log_key, log_value);
    env.insert("FAKE_TOOL_EXIT".to_string(), "7".to_string());
    let config =
        PicoConfig::parse("schema_version = 1\n[build]\ncommand = \"failtool\"\n").unwrap();
    let context = ctx(
        project.path(),
        ProjectKind::Custom,
        Some(&config),
        tool.search_paths(),
        env,
    );
    let outcome = execute_operation(&context, Op::Build).unwrap();
    assert!(!outcome.success());
    assert_eq!(outcome.result.unwrap().exit_code, Some(7));
}

#[test]
fn profile_release_is_rejected_for_config_commands() {
    let project = TempDir::new().unwrap();
    let config = PicoConfig::parse("schema_version = 1\n[build]\ncommand = \"make\"\n").unwrap();
    let tool = FakeTool::new("make");
    let mut context = ctx(
        project.path(),
        ProjectKind::Custom,
        Some(&config),
        tool.search_paths(),
        Default::default(),
    );
    context.profile = Profile::Release;
    let error = execute_operation(&context, Op::Build).unwrap_err();
    assert!(error.to_string().contains("--release"));
}

// ------------------------------------------------------------- stdio policy ----

#[test]
fn run_in_text_mode_inherits_stdio() {
    let project = TempDir::new().unwrap();
    marker_project(project.path(), &[("pyproject.toml", ""), ("main.py", "")]).unwrap();
    let tool = FakeTool::new("python3");
    let mut context = ctx(
        project.path(),
        ProjectKind::Python,
        None,
        tool.search_paths(),
        Default::default(),
    );
    context.stdio = StdioMode::Inherit;
    let outcome = execute_operation(&context, Op::Run).unwrap();
    expect_success(&outcome);
    let result = outcome.result.unwrap();
    assert_eq!(
        result.stdout,
        String::new(),
        "inherited stdio is not captured"
    );
}

#[test]
fn real_path_dirs_are_used_when_search_paths_empty() {
    // When search_paths is empty, resolution falls back to the real PATH —
    // this test only asserts the helper is wired, not that any tool exists.
    assert!(!path_dirs().is_empty() || std::env::var_os("PATH").is_none());
}
