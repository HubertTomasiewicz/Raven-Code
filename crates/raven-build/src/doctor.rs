//! Environment diagnostics: the engine behind `raven-pico doctor` and
//! `raven-pico toolchains`.
//!
//! Doctor checks never install anything, never modify settings and always
//! exit 0 — the report itself is the result (`healthy` is part of the JSON
//! payload).
//!
//! Accuracy rules:
//!
//! * A tool is `ok` **only** when its version probe launched and exited 0 —
//!   never merely because a project type was detected somewhere.
//! * Windows-only tools report `n/a` on other platforms instead of a fake
//!   `ok`.
//! * Every missing tool carries the project kinds that need it plus an
//!   actionable installation hint.

use raven_core::report::CheckDto;

use crate::proc::CancelToken;
use crate::proc::path_dirs;
use crate::toolchain::{Platform, TOOL_CATALOG, ToolSpec, find_msbuild, probe_msbuild_version};

/// A named environment check produced by doctor.
///
/// Status is one of `ok`, `missing`, `n/a` (not applicable on this
/// platform) or `error`.
#[derive(Debug, Clone)]
pub struct Check {
    pub id: String,
    /// `ok` | `missing` | `n/a` | `error`.
    pub status: &'static str,
    pub version: Option<String>,
    pub path: Option<String>,
    pub detail: Option<String>,
    pub hint: Option<String>,
}

impl Check {
    fn to_dto(&self) -> CheckDto {
        CheckDto {
            id: self.id.clone(),
            status: self.status.to_string(),
            version: self.version.clone(),
            path: self.path.clone(),
            detail: self.detail.clone(),
            hint: self.hint.clone(),
        }
    }
}

/// Human-readable list of the project kinds that need a tool, e.g.
/// `"Rust (Cargo) projects"`.
fn used_by_detail(spec: &ToolSpec) -> Option<String> {
    let names: Vec<String> = spec
        .used_by
        .iter()
        .filter_map(|kind| raven_core::kind::ProjectKind::parse(kind))
        .map(|kind| kind.display_name().to_string())
        .collect();
    match names.is_empty() {
        true => None,
        false => Some(format!("required for: {} projects", names.join(", "))),
    }
}

fn check_tool(spec: &ToolSpec, cancel: &CancelToken) -> Check {
    if spec.platform == Platform::WindowsOnly && !cfg!(windows) {
        return Check {
            id: spec.id.to_string(),
            status: "n/a",
            version: None,
            path: None,
            detail: Some("Windows-only tool; not applicable on this platform".to_string()),
            hint: None,
        };
    }
    match crate::toolchain::probe_spec(spec, &[], cancel) {
        Some(info) => Check {
            id: spec.id.to_string(),
            status: "ok",
            version: info.version,
            path: info.path,
            detail: None,
            hint: None,
        },
        None => Check {
            id: spec.id.to_string(),
            status: "missing",
            version: None,
            path: None,
            detail: used_by_detail(spec),
            hint: Some(spec.hint.to_string()),
        },
    }
}

/// Probes every tool in the catalog (and the platform-specific MSBuild
/// lookup) and returns the checks.
pub fn collect_tool_checks(cancel: &CancelToken) -> Vec<Check> {
    let mut checks: Vec<Check> = TOOL_CATALOG
        .iter()
        .map(|spec| check_tool(spec, cancel))
        .collect();

    // MSBuild deserves a dedicated check: the catalog probes PATH, but real
    // MSBuild lives behind vswhere on Windows.
    if cfg!(windows) {
        let msbuild = checks.iter_mut().find(|check| check.id == "msbuild");
        if let Some(msbuild) = msbuild {
            if msbuild.status == "missing" {
                if let Some(path) = find_msbuild() {
                    msbuild.status = "ok";
                    msbuild.path = Some(raven_core::path_to_string(&path));
                    // Version probing is best-effort: the path is already
                    // valuable, a missing version must not hide the tool.
                    msbuild.version = probe_msbuild_version(&path);
                    msbuild.detail =
                        Some("found via Visual Studio installation (vswhere)".to_string());
                }
            }
        }
    }
    checks
}

/// Full doctor report: runtime facts, PATH sanity and tool checks.
pub fn doctor_checks(cancel: &CancelToken) -> Vec<Check> {
    let mut checks = Vec::new();

    checks.push(Check {
        id: "raven-pico".to_string(),
        status: "ok",
        version: Some(env!("CARGO_PKG_VERSION").to_string()),
        path: std::env::current_exe()
            .ok()
            .map(|path| raven_core::path_to_string(&path)),
        detail: Some(format!(
            "running on {} ({})",
            std::env::consts::OS,
            std::env::consts::ARCH
        )),
        hint: None,
    });

    let dirs = path_dirs();
    let dead_entries: Vec<String> = dirs
        .iter()
        .filter(|dir| !dir.is_dir())
        .map(|dir| dir.display().to_string())
        .collect();
    checks.push(Check {
        id: "path".to_string(),
        status: "ok",
        version: None,
        path: None,
        detail: Some(format!(
            "{} directories on PATH{}",
            dirs.len(),
            if dead_entries.is_empty() {
                String::new()
            } else {
                format!(
                    "; {} do not exist: {}",
                    dead_entries.len(),
                    dead_entries.join(", ")
                )
            }
        )),
        hint: None,
    });

    checks.extend(collect_tool_checks(cancel));
    checks
}

/// True when no check errored **and** at least one known toolchain probes
/// as usable (`ok`). Missing tools are warnings, not errors; a machine with
/// no usable toolchain at all is reported as "attention needed" — doctor
/// still exits 0.
pub fn is_healthy(checks: &[Check]) -> bool {
    checks.iter().all(|check| check.status != "error")
        && !checks.is_empty()
        && TOOL_CATALOG.iter().any(|spec| {
            checks
                .iter()
                .any(|check| check.id == spec.id && check.status == "ok")
        })
}

/// Converts checks into the versioned JSON DTOs.
pub fn to_dtos(checks: &[Check]) -> Vec<CheckDto> {
    checks.iter().map(Check::to_dto).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(id: &str, status: &'static str) -> Check {
        Check {
            id: id.to_string(),
            status,
            version: None,
            path: None,
            detail: None,
            hint: None,
        }
    }

    #[test]
    fn healthy_requires_no_errors_and_one_usable_toolchain() {
        let base = [check("raven-pico", "ok"), check("path", "ok")];

        let with_cargo = [
            base[0].clone(),
            base[1].clone(),
            check("cargo", "ok"),
            check("cmake", "missing"),
        ];
        assert!(is_healthy(&with_cargo));

        // No usable toolchain at all -> attention needed.
        let all_missing = [
            base[0].clone(),
            base[1].clone(),
            check("cargo", "missing"),
            check("cmake", "missing"),
        ];
        assert!(!is_healthy(&all_missing));

        // An errored check is never healthy.
        let errored = [
            base[0].clone(),
            base[1].clone(),
            check("cargo", "ok"),
            check("path", "error"),
        ];
        assert!(!is_healthy(&errored));
    }

    #[test]
    fn windows_only_tools_are_not_applicable_off_windows() {
        let cancel = CancelToken::new();
        let msbuild_spec = TOOL_CATALOG
            .iter()
            .find(|spec| spec.id == "msbuild")
            .unwrap();
        let check = check_tool(msbuild_spec, &cancel);
        if cfg!(windows) {
            assert_ne!(check.status, "n/a");
        } else {
            assert_eq!(check.status, "n/a");
            assert!(check.detail.unwrap().contains("Windows-only"));
        }
    }

    #[test]
    fn missing_checks_name_the_project_kinds_that_need_the_tool() {
        // The formatter is deterministic, so assert it directly instead of
        // depending on which tools the CI machine happens to have.
        let spec = TOOL_CATALOG.iter().find(|spec| spec.id == "cmake").unwrap();
        let detail = used_by_detail(spec).unwrap();
        assert!(detail.contains("C/C++ (CMake)"), "detail: {detail}");

        let java = TOOL_CATALOG.iter().find(|spec| spec.id == "java").unwrap();
        let detail = used_by_detail(java).unwrap();
        assert!(
            detail.contains("Maven") && detail.contains("Gradle"),
            "{detail}"
        );
    }

    #[test]
    fn missing_tool_check_carries_hint_and_detail() {
        // Run one catalog probe against an empty search path set is not
        // possible via check_tool (it uses real PATH); assert the structure
        // with a synthetic missing check instead.
        let missing = Check {
            id: "cmake".to_string(),
            status: "missing",
            version: None,
            path: None,
            detail: Some("required for: C/C++ (CMake) projects".to_string()),
            hint: Some("install CMake: https://cmake.org/download/".to_string()),
        };
        let dto = missing.to_dto();
        assert_eq!(dto.status, "missing");
        assert_eq!(
            dto.hint.as_deref(),
            Some("install CMake: https://cmake.org/download/")
        );
    }
}
