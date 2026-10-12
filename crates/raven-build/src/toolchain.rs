//! Toolchain discovery and version probing.
//!
//! Discovery never installs anything and never assumes a tool exists: every
//! probe has a timeout and every missing tool comes with an actionable hint.

use std::path::PathBuf;
use std::time::Duration;

use raven_core::report::ToolInfoDto;

use crate::proc::{CancelToken, ProcessSpec, run_process};

/// Maximum time a single version probe may take.
pub const VERSION_PROBE_TIMEOUT: Duration = Duration::from_secs(4);

/// Cap for captured probe output.
const PROBE_MAX_OUTPUT: usize = 64 * 1024;

/// Which platforms a tool is relevant on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    All,
    WindowsOnly,
}

/// Description of a known tool for discovery, `doctor` and `toolchains`.
pub struct ToolSpec {
    /// Stable id, e.g. `"cargo"`.
    pub id: &'static str,
    /// Program names to probe, in order.
    pub programs: &'static [&'static str],
    /// Arguments used to obtain the version.
    pub version_args: &'static [&'static str],
    pub platform: Platform,
    /// Project kinds (canonical ids, see [`raven_core::kind::ProjectKind`])
    /// that need this tool — used for actionable doctor guidance.
    pub used_by: &'static [&'static str],
    /// Actionable hint when the tool is missing.
    pub hint: &'static str,
}

/// The catalog of tools Raven Pico knows how to discover.
pub const TOOL_CATALOG: &[ToolSpec] = &[
    ToolSpec {
        id: "cargo",
        programs: &["cargo"],
        version_args: &["--version"],
        platform: Platform::All,
        used_by: &["rust"],
        hint: "install Rust via rustup: https://rustup.rs",
    },
    ToolSpec {
        id: "rustc",
        programs: &["rustc"],
        version_args: &["--version"],
        platform: Platform::All,
        used_by: &["rust"],
        hint: "installed together with cargo (https://rustup.rs)",
    },
    ToolSpec {
        id: "cmake",
        programs: &["cmake"],
        version_args: &["--version"],
        platform: Platform::All,
        used_by: &["cmake"],
        hint: "install CMake: https://cmake.org/download/",
    },
    ToolSpec {
        id: "ctest",
        programs: &["ctest"],
        version_args: &["--version"],
        platform: Platform::All,
        used_by: &["cmake"],
        hint: "shipped with CMake: https://cmake.org/download/",
    },
    ToolSpec {
        id: "dotnet",
        programs: &["dotnet"],
        version_args: &["--version"],
        platform: Platform::All,
        used_by: &["dotnet", "visual-studio"],
        hint: "install the .NET SDK: https://dotnet.microsoft.com/download",
    },
    ToolSpec {
        id: "msbuild",
        programs: &["MSBuild.exe"],
        version_args: &["-version"],
        platform: Platform::WindowsOnly,
        used_by: &["visual-studio"],
        hint: "install Visual Studio 2022 or Build Tools with the 'Desktop development with C++' workload; \
               MSBuild is located via vswhere. SDK-style projects can also be built with the .NET SDK (dotnet)",
    },
    ToolSpec {
        id: "python",
        programs: &["python3", "python"],
        version_args: &["--version"],
        platform: Platform::All,
        used_by: &["python"],
        hint: "install Python 3: https://www.python.org/downloads/",
    },
    ToolSpec {
        id: "godot",
        programs: &["godot", "Godot"],
        version_args: &["--version"],
        platform: Platform::All,
        used_by: &["godot"],
        hint: "install Godot 4: https://godotengine.org/download",
    },
    ToolSpec {
        id: "java",
        programs: &["java"],
        version_args: &["-version"],
        platform: Platform::All,
        used_by: &["maven", "gradle"],
        hint: "install a JDK: https://adoptium.net",
    },
    ToolSpec {
        id: "mvn",
        programs: &["mvn"],
        version_args: &["-v"],
        platform: Platform::All,
        used_by: &["maven"],
        hint: "install Maven: https://maven.apache.org/install.html",
    },
    ToolSpec {
        id: "gradle",
        programs: &["gradle"],
        version_args: &["--version"],
        platform: Platform::All,
        used_by: &["gradle"],
        hint: "install Gradle: https://gradle.org/install/ or generate a wrapper with `gradle wrapper`",
    },
    ToolSpec {
        id: "lua",
        programs: &["lua", "lua5.4", "lua5.3", "lua5.2", "luajit"],
        version_args: &["-v"],
        platform: Platform::All,
        used_by: &["lua"],
        hint: "install Lua: https://www.lua.org/ or via your package manager",
    },
    ToolSpec {
        id: "love",
        programs: &["love"],
        version_args: &["--version"],
        platform: Platform::All,
        used_by: &["lua"],
        hint: "install LÖVE to run LÖVE projects: https://love2d.org",
    },
];

/// Extracts a version string from tool output.
///
/// Tools print their version to stdout *or* stderr and in many formats; the
/// parser is intentionally forgiving and falls back to the first non-empty
/// line.
pub fn parse_version(id: &str, output: &str) -> String {
    let first_line = output
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("")
        .to_string();
    match id {
        "cargo" | "rustc" => first_line
            .split_whitespace()
            .nth(1)
            .unwrap_or(&first_line)
            .to_string(),
        "cmake" | "ctest" => first_line
            .strip_prefix("cmake version ")
            .or_else(|| first_line.strip_prefix("ctest version "))
            .unwrap_or(&first_line)
            .to_string(),
        "python" => first_line
            .strip_prefix("Python ")
            .unwrap_or(&first_line)
            .to_string(),
        "java" => {
            // `java -version` prints e.g. openjdk version "21.0.3" ...
            first_line
                .split('"')
                .nth(1)
                .unwrap_or(&first_line)
                .to_string()
        }
        "mvn" => first_line
            .strip_prefix("Apache Maven ")
            .unwrap_or(&first_line)
            .split_whitespace()
            .next()
            .unwrap_or(&first_line)
            .to_string(),
        "gradle" => output
            .lines()
            .map(str::trim)
            .find_map(|line| line.strip_prefix("Gradle "))
            .map(str::to_string)
            .unwrap_or(first_line),
        _ => first_line,
    }
}

/// Probes one tool from [`TOOL_CATALOG`] by id.
///
/// `search_paths` replaces PATH when non-empty (used by tests and the
/// `--tool-arg`-driven integration paths).
pub fn probe_tool(id: &str, search_paths: &[PathBuf], cancel: &CancelToken) -> Option<ToolInfoDto> {
    let spec = TOOL_CATALOG.iter().find(|tool| tool.id == id)?;
    probe_spec(spec, search_paths, cancel)
}

/// Probes a concrete [`ToolSpec`].
///
/// A program only counts as found when its version probe **exits 0** and
/// prints a non-empty version. Probes that fail to launch or exit non-zero
/// (Windows Store `python3.exe` stubs, broken installations) are skipped so
/// doctor never reports a tool as installed when it is not usable.
pub fn probe_spec(
    spec: &ToolSpec,
    search_paths: &[PathBuf],
    cancel: &CancelToken,
) -> Option<ToolInfoDto> {
    if spec.platform == Platform::WindowsOnly && !cfg!(windows) {
        return None;
    }
    for program in spec.programs {
        let mut process_spec = ProcessSpec::new(*program);
        for version_arg in spec.version_args {
            process_spec = process_spec.arg(*version_arg);
        }
        process_spec = process_spec
            .timeout(VERSION_PROBE_TIMEOUT)
            .max_output(PROBE_MAX_OUTPUT);
        if !search_paths.is_empty() {
            process_spec = process_spec.search_paths(search_paths.to_vec());
        }
        let result = run_process(&process_spec, cancel);
        if result.spawn_error.is_some() || result.exit_code != Some(0) {
            continue; // try the next program name
        }
        // Some tools print their version to stderr (java -version, lua -v).
        let combined = format!("{}\n{}", result.stdout, result.stderr);
        let version = parse_version(spec.id, &combined);
        let version = (!version.is_empty()).then_some(version);
        return Some(ToolInfoDto {
            name: spec.id.to_string(),
            version,
            path: result.program.clone().into(),
        });
    }
    None
}

/// Probes the version of an MSBuild found outside PATH (e.g. via vswhere).
///
/// `MSBuild.exe -version -nologo` prints a single version line such as
/// `17.9.8+b1f7a3eab`. Returns `None` when the probe does not exit 0 or
/// prints nothing usable — doctor then reports the path but no version.
pub fn probe_msbuild_version(path: &std::path::Path) -> Option<String> {
    let spec = ProcessSpec::new(path.display().to_string())
        .arg("-version")
        .arg("-nologo")
        .timeout(VERSION_PROBE_TIMEOUT)
        .max_output(PROBE_MAX_OUTPUT);
    let result = run_process(&spec, &CancelToken::new());
    if result.exit_code != Some(0) {
        return None;
    }
    let combined = format!("{}\n{}", result.stdout, result.stderr);
    let version = parse_version("msbuild", &combined);
    (!version.is_empty()).then_some(version)
}

/// Locates `MSBuild.exe` on Windows using vswhere, with a fallback scan of
/// the standard Visual Studio installation roots.
///
/// On non-Windows platforms this returns `None` (the .NET `dotnet msbuild`
/// route is handled by the adapter instead).
pub fn find_msbuild() -> Option<PathBuf> {
    if !cfg!(windows) {
        return None;
    }

    // 1. vswhere — the supported discovery mechanism.
    let program_files_x86 = std::env::var_os("ProgramFiles(x86)")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("C:\\Program Files (x86)"));
    let vswhere = program_files_x86.join(r"Microsoft Visual Studio\Installer\vswhere.exe");
    if vswhere.is_file() {
        let output = std::process::Command::new(&vswhere)
            .args([
                "-latest",
                "-products",
                "*",
                "-requires",
                "Microsoft.Component.MSBuild",
                "-find",
                r"MSBuild\**\Bin\MSBuild.exe",
            ])
            .output();
        if let Ok(output) = output {
            let text = String::from_utf8_lossy(&output.stdout);
            if let Some(first) = text.lines().map(str::trim).find(|line| !line.is_empty()) {
                let path = PathBuf::from(first);
                if path.is_file() {
                    return Some(path);
                }
            }
        }
    }

    // 2. Fallback: scan the standard installation roots (two levels deep).
    for root_key in ["ProgramFiles", "ProgramFiles(x86)"] {
        let Some(root) = std::env::var_os(root_key).map(PathBuf::from) else {
            continue;
        };
        let vs_root = root.join("Microsoft Visual Studio");
        let Ok(years) = std::fs::read_dir(&vs_root) else {
            continue;
        };
        for year in years.flatten() {
            let Ok(editions) = std::fs::read_dir(year.path()) else {
                continue;
            };
            for edition in editions.flatten() {
                let candidate = edition.path().join(r"MSBuild\Current\Bin\MSBuild.exe");
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use testkit::FakeTool;

    #[test]
    fn parses_common_version_formats() {
        assert_eq!(
            parse_version("cargo", "cargo 1.99.0 (abc 2026-01-01)"),
            "1.99.0"
        );
        assert_eq!(parse_version("cmake", "cmake version 3.29.6\n\n"), "3.29.6");
        assert_eq!(parse_version("python", "Python 3.12.4"), "3.12.4");
        assert_eq!(
            parse_version("java", "openjdk version \"21.0.3\" 2024-04-16"),
            "21.0.3"
        );
        assert_eq!(
            parse_version("mvn", "Apache Maven 3.9.6 (abc)\nJava 21"),
            "3.9.6"
        );
        assert_eq!(
            parse_version("gradle", "\nGradle 8.9\nBuild time: ..."),
            "8.9"
        );
        assert_eq!(
            parse_version("godot", "4.3.stable.official"),
            "4.3.stable.official"
        );
        // `MSBuild.exe -version -nologo` prints one bare version line.
        assert_eq!(
            parse_version("msbuild", "17.9.8+b1f7a3eab\n"),
            "17.9.8+b1f7a3eab"
        );
    }

    #[test]
    fn probe_requires_zero_exit_and_non_empty_version() {
        // "python3" exists but its probe exits non-zero (like the Windows
        // Store python3.exe stub): it must be skipped, and the working
        // "python" must win.
        let broken = FakeTool::with_body("python3", "echo broken-probe\nexit 1");
        let working = FakeTool::new("python");
        let mut search_paths = broken.search_paths();
        search_paths.extend(working.search_paths());

        let info = probe_tool("python", &search_paths, &CancelToken::new());
        let info = info.expect("the working python must be found");
        assert_eq!(info.version.as_deref(), Some("fake python ok"));
        assert!(info.path.unwrap().contains(working.name()));
    }

    #[test]
    fn probe_msbuild_version_parses_the_version_line() {
        let fake = FakeTool::new("msbuild-fake");
        let version = probe_msbuild_version(fake.program()).expect("fake msbuild probes");
        assert!(version.starts_with("fake"), "got: {version}");

        assert_eq!(
            probe_msbuild_version(std::path::Path::new("/definitely/not/msbuild")),
            None
        );
    }

    #[test]
    fn used_by_kinds_are_valid_project_kinds() {
        for spec in TOOL_CATALOG {
            assert!(!spec.used_by.is_empty(), "{} must declare used_by", spec.id);
            for kind_id in spec.used_by {
                assert!(
                    raven_core::kind::ProjectKind::parse(kind_id).is_some(),
                    "{} declares invalid kind {kind_id}",
                    spec.id
                );
            }
        }
    }

    #[test]
    fn probe_of_missing_tool_is_none() {
        let empty = tempfile::tempdir().unwrap();
        let info = probe_tool("cargo", &[empty.path().to_path_buf()], &CancelToken::new());
        assert!(info.is_none());
    }

    #[test]
    fn windows_only_tools_are_skipped_off_windows() {
        if cfg!(windows) {
            return;
        }
        let empty = tempfile::tempdir().unwrap();
        assert!(
            probe_tool(
                "msbuild",
                &[empty.path().to_path_buf()],
                &CancelToken::new()
            )
            .is_none(),
            "msbuild must not be probed on non-Windows"
        );
    }

    #[test]
    fn catalog_has_no_duplicate_ids() {
        let mut ids: Vec<_> = TOOL_CATALOG.iter().map(|tool| tool.id).collect();
        let count = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), count);
    }
}
