//! The versioned `raven-pico.toml` project configuration file.
//!
//! Raven Pico works with native project files by default. `raven-pico.toml`
//! is **optional** and exists for projects that need explicit build, test,
//! run or clean commands (or that Pico cannot detect at all — see
//! [`ProjectKind::Custom`](crate::kind::ProjectKind::Custom)).
//!
//! # Security
//!
//! Commands in this file are executed **as-is, without a shell**, whenever
//! you run `raven-pico build|test|run|clean` in the project. That is
//! arbitrary project code execution — review a `raven-pico.toml` from an
//! untrusted source before using it. Pico never installs anything and never
//! runs commands on its own initiative.

use std::path::{Path, PathBuf};
use std::{collections::BTreeMap, fmt};

use crate::error::PicoError;
use crate::kind::ProjectKind;

/// Current configuration schema version produced by `raven-pico init`.
pub const SCHEMA_VERSION: u64 = 1;

/// Name of the optional project configuration file.
pub const CONFIG_FILE_NAME: &str = "raven-pico.toml";

/// A single configured command.
///
/// `command` is a **program** (not a shell line): it is invoked directly,
/// without shell interpretation. Arguments go into [`CmdSpec::args`], one
/// argument per array element, so paths with spaces need no quoting.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CmdSpec {
    /// Program to execute (absolute path, path relative to the project root,
    /// or a bare name resolved from PATH).
    pub command: String,
    /// Argument list passed verbatim.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    /// Working directory relative to the project root (defaults to the root).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
}

/// `[project]` section.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ProjectSection {
    /// Optional display name (used in reports and future Raven UIs).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Optional explicit project kind (e.g. `"rust"`). Overrides detection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

/// `[cmake]` section.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CmakeSection {
    /// Build directory relative to the project root. Defaults to `build`.
    #[serde(default = "default_cmake_build_dir")]
    pub build_dir: String,
}

impl Default for CmakeSection {
    fn default() -> Self {
        CmakeSection {
            build_dir: default_cmake_build_dir(),
        }
    }
}

fn default_cmake_build_dir() -> String {
    "build".to_string()
}

/// Root structure of `raven-pico.toml` (schema version 1).
///
/// Unknown fields are rejected so typos in section or key names are caught
/// early; schema evolution happens under a new `schema_version`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PicoConfig {
    /// Must be `1` for this release.
    pub schema_version: u64,
    #[serde(default)]
    pub project: ProjectSection,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build: Option<CmdSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub test: Option<CmdSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run: Option<CmdSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clean: Option<CmdSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cmake: Option<CmakeSection>,
    /// Extra environment variables for commands Pico spawns in this project.
    /// Values are never logged.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
}

impl PicoConfig {
    /// Parses a configuration from TOML text and validates it.
    pub fn parse(text: &str) -> Result<PicoConfig, PicoError> {
        let config: PicoConfig = toml::from_str(text).map_err(|e| PicoError::ConfigInvalid {
            message: format!("could not parse {CONFIG_FILE_NAME}: {e}"),
        })?;
        config.validate()?;
        Ok(config)
    }

    /// Validates semantic constraints (schema version, kind name, commands).
    pub fn validate(&self) -> Result<(), PicoError> {
        if self.schema_version > SCHEMA_VERSION {
            return Err(PicoError::ConfigInvalid {
                message: format!(
                    "unsupported schema_version {} (this build supports schema_version {SCHEMA_VERSION}); \
                     upgrade raven-pico to use this project",
                    self.schema_version
                ),
            });
        }
        if self.schema_version == 0 {
            return Err(PicoError::ConfigInvalid {
                message: "schema_version must be 1 or higher".to_string(),
            });
        }
        if let Some(kind) = &self.project.kind {
            if ProjectKind::parse(kind).is_none() {
                return Err(PicoError::ConfigInvalid {
                    message: format!(
                        "invalid [project] kind '{kind}'; expected one of: {}",
                        ProjectKind::aliases()
                    ),
                });
            }
        }
        for (op, spec) in self.operations() {
            if spec.command.trim().is_empty() {
                return Err(PicoError::ConfigInvalid {
                    message: format!("[{op}] command must not be empty"),
                });
            }
        }
        Ok(())
    }

    /// The configured operations, in stable order.
    pub fn operations(&self) -> Vec<(&'static str, &CmdSpec)> {
        let mut out = Vec::new();
        if let Some(s) = &self.build {
            out.push(("build", s));
        }
        if let Some(s) = &self.test {
            out.push(("test", s));
        }
        if let Some(s) = &self.run {
            out.push(("run", s));
        }
        if let Some(s) = &self.clean {
            out.push(("clean", s));
        }
        out
    }

    /// The explicit project kind, if valid.
    pub fn kind_override(&self) -> Option<ProjectKind> {
        self.project.kind.as_deref().and_then(ProjectKind::parse)
    }

    /// Names of the operation sections present in this file.
    pub fn defined_sections(&self) -> Vec<String> {
        self.operations()
            .into_iter()
            .map(|(op, _)| op.to_string())
            .collect()
    }
}

/// Result of looking for `raven-pico.toml` in a directory.
///
/// Detection is graceful: a broken configuration is reported as a status
/// instead of aborting detection, while operations refuse to run until the
/// file is fixed.
#[derive(Debug, Clone)]
pub struct LoadedConfig {
    /// Where the file was found.
    pub path: PathBuf,
    /// The parsed configuration, present only when the status is `Ok`.
    pub config: Option<PicoConfig>,
    /// How loading went.
    pub status: ConfigStatus,
}

impl LoadedConfig {
    /// Returns the parsed config, mapping a bad status to a [`PicoError`].
    pub fn require(&self) -> Result<&PicoConfig, PicoError> {
        match (&self.status, &self.config) {
            (ConfigStatus::Ok, Some(config)) => Ok(config),
            (ConfigStatus::ParseError { message }, _) => Err(PicoError::ConfigInvalid {
                message: message.clone(),
            }),
            (ConfigStatus::UnsupportedVersion { found }, _) => Err(PicoError::ConfigInvalid {
                message: format!(
                    "unsupported {CONFIG_FILE_NAME} schema_version {found} \
                     (this build supports schema_version {SCHEMA_VERSION})"
                ),
            }),
            _ => Err(PicoError::Internal {
                message: "LoadedConfig has Ok status but no config".to_string(),
            }),
        }
    }
}

/// Outcome of attempting to load a configuration file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigStatus {
    Ok,
    ParseError { message: String },
    UnsupportedVersion { found: u64 },
}

impl ConfigStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            ConfigStatus::Ok => "ok",
            ConfigStatus::ParseError { .. } => "parse_error",
            ConfigStatus::UnsupportedVersion { .. } => "unsupported_version",
        }
    }
}

impl fmt::Display for ConfigStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigStatus::Ok => write!(f, "ok"),
            ConfigStatus::ParseError { message } => write!(f, "parse error: {message}"),
            ConfigStatus::UnsupportedVersion { found } => {
                write!(f, "unsupported schema_version {found}")
            }
        }
    }
}

/// Reads `raven-pico.toml` from `dir`, if present.
///
/// * `Ok(None)` — no configuration file exists (perfectly normal).
/// * `Ok(Some(loaded))` — file found; inspect [`LoadedConfig::status`].
/// * `Err` — filesystem-level failure (permissions, I/O).
pub fn read_config(dir: &Path) -> std::io::Result<Option<LoadedConfig>> {
    let path = dir.join(CONFIG_FILE_NAME);
    if !path.exists() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&path)?;
    let (config, status) = match PicoConfig::parse(&text) {
        Ok(config) => (Some(config), ConfigStatus::Ok),
        Err(err) => {
            let message = err.to_string();
            // Distinguish "newer schema" from other parse errors for a better UX.
            if message.contains("unsupported schema_version") {
                let found = message
                    .split("schema_version ")
                    .nth(1)
                    .and_then(|rest| rest.split_whitespace().next())
                    .and_then(|token| token.parse::<u64>().ok())
                    .unwrap_or(0);
                (None, ConfigStatus::UnsupportedVersion { found })
            } else {
                (None, ConfigStatus::ParseError { message })
            }
        }
    };
    Ok(Some(LoadedConfig {
        path,
        config,
        status,
    }))
}

/// Generates the commented `raven-pico.toml` template written by `init`.
///
/// The kind is pre-selected when detection (or `--kind`) provides one.
pub fn template(kind: Option<ProjectKind>) -> String {
    let kind_line = match kind {
        Some(kind) => format!("kind = \"{}\"", kind.as_str()),
        None => "# kind = \"rust\"  # optional: rust, cmake, visual-studio, dotnet, python, godot, maven, gradle, lua, custom".to_string(),
    };
    format!(
        r#"# raven-pico.toml — Raven Pico project configuration (schema version {SCHEMA_VERSION})
#
# SECURITY: the commands below are executed directly (without a shell) when
# you run `raven-pico build|test|run|clean` in this project. That is arbitrary
# project code execution - review this file before trusting it.
#
# This file is OPTIONAL. Raven Pico detects and drives native project files
# (Cargo.toml, CMakeLists.txt, .sln, pyproject.toml, ...) automatically.

schema_version = {SCHEMA_VERSION}

[project]
# Optional display name used by Raven tools.
# name = "My Project"
{kind_line}

# Each section is optional. When present, it takes precedence over the
# built-in adapter for that operation. `command` is a program (never a shell
# line); provide one argument per array element.

#[build]
#command = "make"
#args = ["all"]

#[test]
#command = "python"
#args = ["-m", "pytest", "-q"]

#[run]
#command = "python"
#args = ["main.py"]

#[clean]
#command = "make"
#args = ["clean"]

# Adapter-specific settings.

#[cmake]
#build_dir = "build"    # relative to this file

# Extra environment variables for commands Raven Pico spawns in this project.
# Values are never included in logs.

#[env]
#MY_VAR = "value"
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_config() {
        let config = PicoConfig::parse("schema_version = 1").unwrap();
        assert_eq!(config.schema_version, 1);
        assert!(config.operations().is_empty());
    }

    #[test]
    fn parses_full_config() {
        let text = r#"
schema_version = 1
[project]
name = "Demo"
kind = "rust"
[build]
command = "make"
args = ["all"]
[run]
command = "./app"
[env]
KEY = "value"
[cmake]
build_dir = "out"
"#;
        let config = PicoConfig::parse(text).unwrap();
        assert_eq!(config.kind_override(), Some(ProjectKind::Rust));
        assert_eq!(config.defined_sections(), vec!["build", "run"]);
        assert_eq!(config.cmake.unwrap().build_dir, "out");
        assert_eq!(config.env.get("KEY").map(String::as_str), Some("value"));
    }

    #[test]
    fn rejects_unknown_fields() {
        let err = PicoConfig::parse("schema_version = 1\nnope = true").unwrap_err();
        assert!(matches!(err, PicoError::ConfigInvalid { .. }));
    }

    #[test]
    fn rejects_future_schema_version() {
        let err = PicoConfig::parse("schema_version = 99").unwrap_err();
        assert!(err.to_string().contains("schema_version 99"));
    }

    #[test]
    fn rejects_unknown_kind() {
        let err =
            PicoConfig::parse("schema_version = 1\n[project]\nkind = \"fortran\"").unwrap_err();
        assert!(err.to_string().contains("fortran"));
    }

    #[test]
    fn rejects_empty_command() {
        let err = PicoConfig::parse("schema_version = 1\n[run]\ncommand = \" \"").unwrap_err();
        assert!(err.to_string().contains("empty"));
    }
}
