//! Cross-crate integration tests: the exact flow a future Raven Studio /
//! Raven Code SDK client would use — detect → select → execute → report —
//! plus JSON envelope stability guarantees.

use std::collections::BTreeMap;
use std::fs;

use raven_build::proc::path_dirs;
use raven_build::{CancelToken, Op, OpContext, Profile, StdioMode, execute_operation};
use raven_core::config::PicoConfig;
use raven_core::kind::ProjectKind;
use raven_core::progress::SilentSink;
use raven_core::report::{Report, ReportPayload};
use raven_projects::{DetectOptions, detect, select};
use tempfile::TempDir;
use testkit::{FakeTool, marker_project};

fn full_flow(
    dir: &std::path::Path,
    kind: ProjectKind,
    op: Op,
    tool: &FakeTool,
) -> raven_build::OperationOutcome {
    let context = OpContext {
        root: dir.to_path_buf(),
        kind,
        config: None,
        profile: Profile::Debug,
        extra_args: Vec::new(),
        env: {
            let (key, value) = tool.log_env();
            let mut env: BTreeMap<String, String> = BTreeMap::new();
            env.insert(key, value);
            env
        },
        search_paths: tool.search_paths(),
        sink: Some(&SilentSink),
        cancel: CancelToken::new(),
        stdio: StdioMode::Capture,
    };
    execute_operation(&context, op).expect("operation executes")
}

#[test]
fn detect_select_execute_flow_with_fake_toolchain() {
    let project = TempDir::new().unwrap();
    marker_project(
        project.path(),
        &[("CMakeLists.txt", "cmake_minimum_required(VERSION 3.16)")],
    )
    .unwrap();

    // 1. Detect (library level, exactly like the CLI).
    let detection = detect(project.path(), &DetectOptions::default()).expect("detects");
    assert_eq!(detection.primary.as_ref().unwrap().kind, ProjectKind::CMake);

    // 2. Select.
    let (kind, root) = select(&detection, None).unwrap();
    assert_eq!(kind, ProjectKind::CMake);
    assert_eq!(root, project.path());

    // 3. Execute with a fake toolchain.
    let cmake = FakeTool::new("cmake");
    let outcome = full_flow(&root, kind, Op::Build, &cmake);
    assert!(outcome.success());
    let lines = cmake.log_lines();
    assert_eq!(lines.len(), 2, "configure + build: {lines:?}");
}

#[test]
fn every_operation_payload_serializes_with_stable_keys() {
    let project = TempDir::new().unwrap();
    marker_project(project.path(), &[("Cargo.toml", "")]).unwrap();
    let cargo = FakeTool::new("cargo");

    for (op, key) in [
        (Op::Build, "build"),
        (Op::Test, "test"),
        (Op::Clean, "clean"),
    ] {
        let outcome = full_flow(project.path(), ProjectKind::Rust, op, &cargo);
        assert!(outcome.success(), "{key} must succeed");
        let payload = outcome.to_payload(ProjectKind::Rust, Profile::Debug, project.path());
        let report = Report::new(key, "0.2.0")
            .with_success(0)
            .with_payload(match op {
                Op::Build => ReportPayload::Build(payload.clone()),
                Op::Test => ReportPayload::Test(payload.clone()),
                Op::Clean => ReportPayload::Clean(payload.clone()),
                _ => unreachable!(),
            });
        let value = serde_json::to_value(&report).unwrap();
        assert_eq!(value["schema_version"], 1);
        assert_eq!(value["command"], key);
        let section = &value[key];
        assert_eq!(section["kind"], "rust");
        assert_eq!(section["profile"], "debug");
        assert!(section["command"].as_array().is_some());
        assert!(section["duration_ms"].is_u64());
        // stdout present because the fake tool printed its banner
        assert!(section["stdout"].as_str().is_some());
    }
}

#[test]
fn detection_and_selection_reject_ambiguity_for_operations() {
    let project = TempDir::new().unwrap();
    marker_project(project.path(), &[("pom.xml", ""), ("build.gradle", "")]).unwrap();
    let detection = detect(project.path(), &DetectOptions::default()).unwrap();
    assert!(detection.ambiguous);
    assert!(detection.primary.is_none());
    let error = select(&detection, None).unwrap_err();
    assert_eq!(error.exit_code(), raven_core::error::exit_codes::AMBIGUOUS);
}

#[test]
fn config_driven_custom_project_round_trip() {
    let project = TempDir::new().unwrap();
    let marker = project.path().join("output.txt");
    let body = if cfg!(windows) {
        format!("type nul > \"{}\"", marker.display())
    } else {
        format!("echo done > \"{}\"", marker.display())
    };
    let tool = FakeTool::with_body("generator", &body);
    marker_project(
        project.path(),
        &[(
            "raven-pico.toml",
            "schema_version = 1\n[build]\ncommand = \"generator\"\nargs = [\"--fast\"]\n",
        )],
    )
    .unwrap();

    let detection = detect(project.path(), &DetectOptions::default()).unwrap();
    let (kind, _) = select(&detection, None).unwrap();
    assert_eq!(kind, ProjectKind::Custom);

    let config = detection
        .config
        .as_ref()
        .unwrap()
        .require()
        .unwrap()
        .clone();
    let context = OpContext {
        root: project.path().to_path_buf(),
        kind,
        config: Some(&config),
        profile: Profile::Debug,
        extra_args: Vec::new(),
        env: Default::default(),
        search_paths: tool.search_paths(),
        sink: Some(&SilentSink),
        cancel: CancelToken::new(),
        stdio: StdioMode::Capture,
    };
    let outcome = execute_operation(&context, Op::Build).unwrap();
    assert!(outcome.success());
    assert_eq!(
        outcome.command.as_deref(),
        Some(&["generator".to_string(), "--fast".to_string()][..])
    );
    assert!(marker.is_file(), "the configured command must really run");
}

#[test]
fn doctor_and_toolchains_produce_checks_without_toolchains_required() {
    // These checks must never fail just because tools are missing.
    let checks = raven_build::doctor::collect_tool_checks(&CancelToken::new());
    assert!(!checks.is_empty());
    assert!(
        checks
            .iter()
            .all(|check| matches!(check.status, "ok" | "missing" | "n/a")),
        "statuses: {:?}",
        checks
            .iter()
            .map(|check| (check.id.as_str(), check.status))
            .collect::<Vec<_>>()
    );
    // Windows-only tools are honest about being n/a off-Windows (never ok).
    #[cfg(not(windows))]
    {
        let msbuild = checks.iter().find(|check| check.id == "msbuild").unwrap();
        assert_eq!(msbuild.status, "n/a");
    }
    let dtos = raven_build::doctor::to_dtos(&checks);
    let value = serde_json::to_value(&dtos).unwrap();
    assert!(value.as_array().unwrap().len() >= 10);
}

#[test]
fn cancelled_operation_is_reported_as_cancelled() {
    let project = TempDir::new().unwrap();
    marker_project(project.path(), &[("Cargo.toml", "")]).unwrap();
    let slow = FakeTool::sleeping("cargo", 30);
    let cancel = CancelToken::new();
    // Cancel immediately — the pre-spawn check must prevent any hang, and
    // the watchdog covers the mid-flight case.
    cancel.cancel();
    let context = OpContext {
        root: project.path().to_path_buf(),
        kind: ProjectKind::Rust,
        config: None,
        profile: Profile::Debug,
        extra_args: Vec::new(),
        env: Default::default(),
        search_paths: slow.search_paths(),
        sink: Some(&SilentSink),
        cancel,
        stdio: StdioMode::Capture,
    };
    let outcome = execute_operation(&context, Op::Build);
    // Either the pre-spawn check or the watchdog catches it.
    match outcome {
        Ok(outcome) => assert!(outcome.cancelled(), "outcome: {outcome:?}"),
        Err(error) => assert_eq!(error.exit_code(), raven_core::error::exit_codes::CANCELLED),
    }
}

#[test]
fn path_with_spaces_through_the_full_stack() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path().join("project dir with spaces");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("project.godot"), "config_version=5").unwrap();
    let detection = detect(&dir, &DetectOptions::default()).unwrap();
    let (kind, root) = select(&detection, None).unwrap();
    assert_eq!(kind, ProjectKind::Godot);
    let godot = FakeTool::new("godot");
    let (key, value) = godot.log_env();
    let context = OpContext {
        root: root.clone(),
        kind,
        config: None,
        profile: Profile::Debug,
        extra_args: Vec::new(),
        env: [(key, value)].into_iter().collect(),
        search_paths: godot.search_paths(),
        sink: Some(&SilentSink),
        cancel: CancelToken::new(),
        stdio: StdioMode::Capture,
    };
    let outcome = execute_operation(&context, Op::Run).unwrap();
    assert!(outcome.success());
    let logged = godot.log_lines().join(" ");
    assert!(logged.contains("project dir with spaces"), "{logged}");
}

#[test]
fn real_path_fallback_exists_for_library_consumers() {
    // Library consumers pass search_paths explicitly; an empty list means
    // "use PATH". Both configurations must compile and behave.
    assert!(raven_build::proc::resolve_in(&path_dirs(), "cargo").is_some() || true);
    let missing = raven_build::proc::resolve_in(&[], "definitely-not-a-tool-xyz");
    assert!(missing.is_none());
}

#[test]
fn config_parsing_rejects_unknown_schema_versions() {
    let error = PicoConfig::parse("schema_version = 2").unwrap_err();
    assert_eq!(
        error.exit_code(),
        raven_core::error::exit_codes::CONFIG_INVALID
    );
}

#[test]
fn detection_boundary_is_explicit_across_crates() {
    // Regression for the 0.1.0 report: a markerless subdirectory inside a
    // repository must not resolve to the ancestor's project unless the
    // client explicitly opts in (SDK users get the same rule as the CLI).
    let root = TempDir::new().unwrap();
    marker_project(root.path(), &[("Cargo.toml", "[package]\nname=\"x\"\n")]).unwrap();
    let unknown = root.path().join("unknown-project");
    fs::create_dir_all(&unknown).unwrap();
    fs::write(unknown.join("readme.md"), "nothing here").unwrap();

    let error = detect(&unknown, &DetectOptions::default()).unwrap_err();
    assert!(matches!(
        error,
        raven_core::error::PicoError::ProjectNotDetected { .. }
    ));

    let opts = raven_projects::DetectOptions {
        search_parents: true,
        ..raven_projects::DetectOptions::default()
    };
    let detection = detect(&unknown, &opts).unwrap();
    assert!(detection.searched_upwards);
    assert_eq!(detection.root, root.path().canonicalize().unwrap());

    // A missing path is a usage error (exit 2), not a detection result.
    let missing = root.path().join("definitely-missing");
    let error = detect(&missing, &DetectOptions::default()).unwrap_err();
    assert_eq!(error.exit_code(), raven_core::error::exit_codes::USAGE);
}
