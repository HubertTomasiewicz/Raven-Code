//! Detection unit tests. Fixtures are created in temporary directories so no
//! real toolchain is needed.

use std::fs;

use crate::{DetectOptions, detect, select};
use raven_core::config::ConfigStatus;
use raven_core::error::PicoError;
use raven_core::kind::{Confidence, ProjectKind};
use tempfile::TempDir;

fn opts() -> DetectOptions {
    DetectOptions::default()
}

type Fixture = (
    &'static [(&'static str, &'static str)],
    ProjectKind,
    &'static str,
);

fn dir_with(files: &[(&str, &str)]) -> (TempDir, std::path::PathBuf) {
    let tmp = TempDir::new().expect("temp dir");
    let dir = tmp.path().join("proj");
    fs::create_dir_all(&dir).expect("mkdir");
    for (name, content) in files {
        fs::write(dir.join(name), content).expect("write marker");
    }
    (tmp, dir)
}

#[test]
fn detects_every_supported_kind() {
    let cases: &[Fixture] = &[
        (
            &[("Cargo.toml", "[package]\nname=\"x\"\nversion=\"0.1.0\"\n")],
            ProjectKind::Rust,
            "Cargo.toml",
        ),
        (
            &[("CMakeLists.txt", "cmake_minimum_required(VERSION 3.16)\n")],
            ProjectKind::CMake,
            "CMakeLists.txt",
        ),
        (&[("App.sln", "")], ProjectKind::VisualStudio, "App.sln"),
        (&[("App.slnx", "")], ProjectKind::VisualStudio, "App.slnx"),
        (
            &[("App.vcxproj", "")],
            ProjectKind::VisualStudio,
            "App.vcxproj",
        ),
        (&[("App.csproj", "")], ProjectKind::DotNet, "App.csproj"),
        (&[("Lib.fsproj", "")], ProjectKind::DotNet, "Lib.fsproj"),
        (
            &[("pyproject.toml", "[project]\nname=\"x\"\n")],
            ProjectKind::Python,
            "pyproject.toml",
        ),
        (
            &[("setup.py", "from setuptools import setup\n")],
            ProjectKind::Python,
            "setup.py",
        ),
        (
            &[("requirements.txt", "rich\n")],
            ProjectKind::Python,
            "requirements.txt",
        ),
        (
            &[("project.godot", "config_version=5\n")],
            ProjectKind::Godot,
            "project.godot",
        ),
        (
            &[("pom.xml", "<project/>\n")],
            ProjectKind::Maven,
            "pom.xml",
        ),
        (&[("build.gradle", "")], ProjectKind::Gradle, "build.gradle"),
        (
            &[("build.gradle.kts", "")],
            ProjectKind::Gradle,
            "build.gradle.kts",
        ),
        (
            &[("settings.gradle", "")],
            ProjectKind::Gradle,
            "settings.gradle",
        ),
        (&[("main.lua", "print(1)\n")], ProjectKind::Lua, "main.lua"),
        (&[("x.rockspec", "")], ProjectKind::Lua, "x.rockspec"),
    ];
    for (files, expected_kind, expected_marker) in cases {
        let (_tmp, dir) = dir_with(files);
        let detection = detect(&dir, &opts()).expect("should detect");
        assert_eq!(detection.root, dir, "root for {expected_kind:?}");
        assert!(!detection.searched_upwards);
        let primary = detection.primary.as_ref().expect("primary candidate");
        assert_eq!(
            primary.kind, *expected_kind,
            "kind for marker {expected_marker}"
        );
        assert!(primary.markers.iter().any(|m| m == expected_marker));
        match *expected_kind {
            ProjectKind::Lua => assert_eq!(primary.confidence, Confidence::Medium),
            _ => assert_eq!(primary.confidence, Confidence::High),
        }
    }
}

#[test]
fn merges_markers_for_the_same_kind() {
    let (_tmp, dir) = dir_with(&[
        ("pyproject.toml", "[project]\nname=\"x\"\n"),
        ("setup.py", ""),
        ("requirements.txt", ""),
    ]);
    let detection = detect(&dir, &opts()).unwrap();
    assert_eq!(detection.candidates.len(), 1);
    let primary = detection.primary.as_ref().unwrap();
    assert_eq!(primary.kind, ProjectKind::Python);
    assert_eq!(primary.markers.len(), 3);
}

#[test]
fn solution_outranks_csproj_but_both_are_reported() {
    let (_tmp, dir) = dir_with(&[("App.sln", ""), ("App.csproj", "")]);
    let detection = detect(&dir, &opts()).unwrap();
    assert!(!detection.ambiguous);
    assert_eq!(
        detection.primary.as_ref().unwrap().kind,
        ProjectKind::VisualStudio
    );
    let kinds: Vec<_> = detection.candidates.iter().map(|c| c.kind).collect();
    assert!(kinds.contains(&ProjectKind::DotNet));
}

#[test]
fn maven_and_gradle_in_the_same_dir_are_ambiguous() {
    let (_tmp, dir) = dir_with(&[("pom.xml", ""), ("build.gradle", "")]);
    let detection = detect(&dir, &opts()).unwrap();
    assert!(detection.ambiguous);
    assert!(detection.primary.is_none());
    let selection = select(&detection, None);
    assert!(matches!(selection, Err(PicoError::AmbiguousProject { .. })));
    let (kind, _) = select(&detection, Some(ProjectKind::Gradle)).unwrap();
    assert_eq!(kind, ProjectKind::Gradle);
}

#[test]
fn cargo_outranks_cmake_by_documented_priority() {
    let (_tmp, dir) = dir_with(&[("Cargo.toml", ""), ("CMakeLists.txt", "")]);
    let detection = detect(&dir, &opts()).unwrap();
    assert!(!detection.ambiguous);
    assert_eq!(detection.primary.as_ref().unwrap().kind, ProjectKind::Rust);
    assert_eq!(detection.candidates.len(), 2);
}

#[test]
fn config_kind_override_forces_primary() {
    let config = "schema_version = 1\n[project]\nkind = \"cmake\"\n";
    let (_tmp, dir) = dir_with(&[("Cargo.toml", ""), ("raven-pico.toml", config)]);
    let detection = detect(&dir, &opts()).unwrap();
    let primary = detection.primary.as_ref().unwrap();
    assert_eq!(primary.kind, ProjectKind::CMake);
    assert_eq!(primary.confidence, Confidence::Explicit);
    let (kind, _) = select(&detection, None).unwrap();
    assert_eq!(kind, ProjectKind::CMake);
}

#[test]
fn config_with_commands_only_becomes_custom() {
    let config = "schema_version = 1\n[build]\ncommand = \"make\"\n";
    let (_tmp, dir) = dir_with(&[("raven-pico.toml", config)]);
    let detection = detect(&dir, &opts()).unwrap();
    let primary = detection.primary.as_ref().unwrap();
    assert_eq!(primary.kind, ProjectKind::Custom);
    assert_eq!(primary.confidence, Confidence::Explicit);
}

#[test]
fn plain_config_without_commands_is_not_custom() {
    let config = "schema_version = 1\n[project]\nname = \"JustDocs\"\n";
    let (_tmp, dir) = dir_with(&[("raven-pico.toml", config)]);
    let detection = detect(&dir, &opts()).unwrap();
    assert!(detection.candidates.is_empty());
    assert!(detection.primary.is_none());
    assert_eq!(detection.config.as_ref().unwrap().status, ConfigStatus::Ok);
}

#[test]
fn malformed_config_is_reported_gracefully() {
    let (_tmp, dir) = dir_with(&[("Cargo.toml", ""), ("raven-pico.toml", "not = [valid")]);
    let detection = detect(&dir, &opts()).unwrap();
    let loaded = detection.config.as_ref().unwrap();
    assert!(matches!(loaded.status, ConfigStatus::ParseError { .. }));
    // Detect still succeeds and reports the native candidate...
    assert_eq!(detection.primary.as_ref().unwrap().kind, ProjectKind::Rust);
    // ...but operations refuse until the config is fixed.
    assert!(matches!(
        select(&detection, None),
        Err(PicoError::ConfigInvalid { .. })
    ));
}

#[test]
fn future_config_version_is_reported_gracefully() {
    let (_tmp, dir) = dir_with(&[("raven-pico.toml", "schema_version = 42")]);
    let detection = detect(&dir, &opts()).unwrap();
    assert!(matches!(
        detection.config.as_ref().unwrap().status,
        ConfigStatus::UnsupportedVersion { found: 42 }
    ));
}

#[test]
fn unknown_project_is_an_error() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path().join("empty");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("readme.md"), "nothing here").unwrap();
    let err = detect(&dir, &opts()).unwrap_err();
    assert!(matches!(err, PicoError::ProjectNotDetected { .. }));
    assert_eq!(err.exit_code(), raven_core::error::exit_codes::NOT_DETECTED);
}

#[test]
fn missing_directory_and_file_paths_are_usage_errors() {
    let tmp = TempDir::new().unwrap();
    let missing = tmp.path().join("does-not-exist");
    let err = detect(&missing, &opts()).unwrap_err();
    // A missing path must be a PATH error — never a detection result.
    assert!(matches!(err, PicoError::PathNotFound { .. }));
    assert_eq!(err.code().as_str(), "invalid_path");
    assert_eq!(err.exit_code(), raven_core::error::exit_codes::USAGE);
    assert_ne!(err.code().as_str(), "not_detected");

    let file = tmp.path().join("afile.txt");
    fs::write(&file, "x").unwrap();
    assert!(matches!(
        detect(&file, &opts()),
        Err(PicoError::NotADirectory { .. })
    ));
}

#[test]
fn markerless_directory_never_reports_an_ancestor_project_by_default() {
    // Regression: `unknown-project` inside a repository used to detect the
    // ancestor's Cargo project. The default must be the opposite.
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("my repo");
    let unknown = root.join("unknown-project");
    fs::create_dir_all(&unknown).unwrap();
    fs::write(root.join("Cargo.toml"), "[package]\nname=\"x\"\n").unwrap();
    fs::write(unknown.join("readme.md"), "nothing here").unwrap();

    let err = detect(&unknown, &opts()).unwrap_err();
    assert!(matches!(err, PicoError::ProjectNotDetected { .. }));

    // Explicit opt-in discovers the ancestor and says so.
    let up = DetectOptions {
        search_parents: true,
        ..DetectOptions::default()
    };
    let detection = detect(&unknown, &up).unwrap();
    assert_eq!(detection.root, root);
    assert!(detection.searched_upwards);
    assert_eq!(detection.searched, unknown);
}

#[test]
fn supplied_directory_with_its_own_marker_wins_over_ancestors() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("repo");
    let nested = root.join("examples/py-app");
    fs::create_dir_all(&nested).unwrap();
    fs::write(root.join("Cargo.toml"), "").unwrap();
    fs::write(nested.join("pyproject.toml"), "[project]\nname=\"x\"\n").unwrap();

    // Even with parent search enabled, the start directory's own markers win.
    let up = DetectOptions {
        search_parents: true,
        ..DetectOptions::default()
    };
    let detection = detect(&nested, &up).unwrap();
    assert_eq!(detection.root, nested);
    assert!(!detection.searched_upwards);
    assert_eq!(
        detection.primary.as_ref().unwrap().kind,
        ProjectKind::Python
    );
}

#[test]
fn config_only_directory_is_its_own_root() {
    // A raven-pico.toml counts as a marker: the start directory wins and no
    // ancestor search happens even when it would find something.
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("repo");
    let nested = root.join("tools");
    fs::create_dir_all(&nested).unwrap();
    fs::write(root.join("Cargo.toml"), "").unwrap();
    fs::write(
        nested.join("raven-pico.toml"),
        "schema_version = 1\n[build]\ncommand = \"make\"\n",
    )
    .unwrap();

    let detection = detect(&nested, &opts()).unwrap();
    assert_eq!(detection.root, nested);
    assert!(!detection.searched_upwards);
    assert_eq!(
        detection.primary.as_ref().unwrap().kind,
        ProjectKind::Custom
    );
}

#[test]
fn paths_with_spaces_work() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path().join("my project (copy) with spaces");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("Cargo.toml"), "[package]\nname=\"x\"\n").unwrap();
    let detection = detect(&dir, &opts()).unwrap();
    assert_eq!(detection.root, dir);
    assert_eq!(detection.primary.as_ref().unwrap().kind, ProjectKind::Rust);
}

#[test]
fn parent_search_is_an_explicit_opt_in() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("my proj");
    let nested = root.join("src/commands");
    fs::create_dir_all(&nested).unwrap();
    fs::write(root.join("Cargo.toml"), "[package]\nname=\"x\"\n").unwrap();

    // Default: no ancestor search — the markerless start directory is an
    // honest "not detected" instead of a silent ancestor project.
    assert!(detect(&nested, &opts()).is_err());

    // Opt-in: walk up, find the root, and report that you did.
    let up = DetectOptions {
        search_parents: true,
        ..DetectOptions::default()
    };
    let detection = detect(&nested, &up).unwrap();
    assert_eq!(detection.root, root);
    assert!(detection.searched_upwards);
    assert_eq!(detection.searched, nested);
}

#[test]
fn parent_search_respects_the_bound() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("proj");
    let nested = root.join("a/b/c/d");
    fs::create_dir_all(&nested).unwrap();
    fs::write(root.join("Cargo.toml"), "").unwrap();

    // max_upwards = 2 stops before reaching the root two+ levels above.
    let bounded = DetectOptions {
        search_parents: true,
        max_upwards: 2,
    };
    assert!(detect(&nested, &bounded).is_err());

    let unbounded = DetectOptions {
        search_parents: true,
        max_upwards: 32,
    };
    let detection = detect(&nested, &unbounded).unwrap();
    assert_eq!(detection.root, root);
}

#[test]
fn detection_payload_is_valid_json() {
    let (_tmp, dir) = dir_with(&[("Cargo.toml", "")]);
    let detection = detect(&dir, &opts()).unwrap();
    let payload = detection.to_payload();
    let value = serde_json::to_value(&payload).unwrap();
    assert_eq!(value["primary"]["kind"], "rust");
    assert_eq!(value["ambiguous"], false);
    assert_eq!(value["root"], raven_core::path_to_string(&dir));
}

#[test]
fn detection_uses_clean_absolute_paths() {
    // Roots are absolute and serialized through raven_core::path_to_string
    // (Windows verbatim prefixes removed when safe). On Unix the cleaned
    // canonical form equals the canonical form itself.
    let (_tmp, dir) = dir_with(&[("CMakeLists.txt", "")]);
    let detection = detect(&dir, &opts()).unwrap();
    let canonical = std::fs::canonicalize(&dir).unwrap();
    assert_eq!(detection.root, canonical);
    assert_eq!(
        detection.to_payload().root.unwrap(),
        raven_core::path_to_string(&canonical)
    );
}
