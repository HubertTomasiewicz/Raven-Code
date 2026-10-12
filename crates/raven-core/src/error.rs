//! The unified error type for Raven Pico.
//!
//! Every ordinary failure is represented as a [`PicoError`] — the library never
//! panics on expected failure modes (missing tools, bad paths, malformed
//! project files, non-zero exits). Each error carries a stable
//! [`ErrorCode`](ErrorCode) for machine consumption and a documented process
//! exit code (see [`exit_codes`]).

use std::path::PathBuf;

/// Documented process exit codes used by the `raven-pico` CLI.
///
/// These are part of the public contract; see `docs/json-schema.md`.
pub mod exit_codes {
    /// Operation completed successfully.
    pub const SUCCESS: i32 = 0;
    /// Operation failed: the invoked tool returned a non-zero exit code, or a
    /// generic operation failure occurred.
    pub const FAILED: i32 = 1;
    /// Usage error: invalid arguments or an invalid user-supplied path.
    pub const USAGE: i32 = 2;
    /// No project could be detected in the given path (nor in its ancestors
    /// when parent search was explicitly enabled).
    pub const NOT_DETECTED: i32 = 3;
    /// A required toolchain was not found on PATH.
    pub const TOOL_MISSING: i32 = 4;
    /// Multiple project types were detected and no unambiguous primary exists.
    pub const AMBIGUOUS: i32 = 5;
    /// The `raven-pico.toml` configuration is invalid or unsupported.
    pub const CONFIG_INVALID: i32 = 6;
    /// The operation was cancelled.
    pub const CANCELLED: i32 = 7;
    /// Internal error — likely a bug; please report it.
    pub const INTERNAL: i32 = 8;
    /// The operation is not supported for this project type.
    pub const UNSUPPORTED: i32 = 9;
    /// Conflict: e.g. `init` refusing to overwrite an existing configuration.
    pub const CONFLICT: i32 = 10;
}

/// Stable machine-readable error codes used in JSON reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    InvalidPath,
    NotDetected,
    Ambiguous,
    ToolMissing,
    ToolFailed,
    Unsupported,
    ConfigInvalid,
    ConfigExists,
    Cancelled,
    Io,
    Internal,
}

impl ErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorCode::InvalidPath => "invalid_path",
            ErrorCode::NotDetected => "not_detected",
            ErrorCode::Ambiguous => "ambiguous_project",
            ErrorCode::ToolMissing => "tool_missing",
            ErrorCode::ToolFailed => "tool_failed",
            ErrorCode::Unsupported => "unsupported_operation",
            ErrorCode::ConfigInvalid => "config_invalid",
            ErrorCode::ConfigExists => "config_exists",
            ErrorCode::Cancelled => "cancelled",
            ErrorCode::Io => "io_error",
            ErrorCode::Internal => "internal_error",
        }
    }
}

/// The single error type shared by all Raven Pico crates.
#[derive(Debug, thiserror::Error)]
pub enum PicoError {
    #[error("path does not exist: {path}")]
    PathNotFound { path: PathBuf },

    #[error("not a directory: {path}")]
    NotADirectory { path: PathBuf },

    #[error("no recognizable project found in {path}")]
    ProjectNotDetected { path: PathBuf },

    #[error("multiple project types detected in {path}: {kinds}")]
    AmbiguousProject { path: PathBuf, kinds: String },

    #[error("required tool '{tool}' was not found")]
    ToolMissing { tool: String, hint: String },

    #[error("tool '{tool}' failed with exit code {exit_code}")]
    ToolFailed { tool: String, exit_code: i32 },

    #[error("operation '{operation}' is not supported for {kind}: {reason}")]
    UnsupportedOperation {
        operation: String,
        kind: String,
        reason: String,
    },

    #[error("invalid configuration: {message}")]
    ConfigInvalid { message: String },

    #[error("configuration file already exists: {path}")]
    ConfigExists { path: PathBuf },

    #[error("operation cancelled")]
    Cancelled,

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("internal error: {message}")]
    Internal { message: String },
}

impl PicoError {
    /// Machine-readable error code for JSON reports.
    pub fn code(&self) -> ErrorCode {
        match self {
            PicoError::PathNotFound { .. } | PicoError::NotADirectory { .. } => {
                ErrorCode::InvalidPath
            }
            PicoError::ProjectNotDetected { .. } => ErrorCode::NotDetected,
            PicoError::AmbiguousProject { .. } => ErrorCode::Ambiguous,
            PicoError::ToolMissing { .. } => ErrorCode::ToolMissing,
            PicoError::ToolFailed { .. } => ErrorCode::ToolFailed,
            PicoError::UnsupportedOperation { .. } => ErrorCode::Unsupported,
            PicoError::ConfigInvalid { .. } => ErrorCode::ConfigInvalid,
            PicoError::ConfigExists { .. } => ErrorCode::ConfigExists,
            PicoError::Cancelled => ErrorCode::Cancelled,
            PicoError::Io(_) => ErrorCode::Io,
            PicoError::Internal { .. } => ErrorCode::Internal,
        }
    }

    /// Documented CLI exit code for this error.
    pub fn exit_code(&self) -> i32 {
        match self {
            PicoError::PathNotFound { .. } | PicoError::NotADirectory { .. } => exit_codes::USAGE,
            PicoError::ProjectNotDetected { .. } => exit_codes::NOT_DETECTED,
            PicoError::AmbiguousProject { .. } => exit_codes::AMBIGUOUS,
            PicoError::ToolMissing { .. } => exit_codes::TOOL_MISSING,
            PicoError::ToolFailed { .. } => exit_codes::FAILED,
            PicoError::UnsupportedOperation { .. } => exit_codes::UNSUPPORTED,
            PicoError::ConfigInvalid { .. } => exit_codes::CONFIG_INVALID,
            PicoError::ConfigExists { .. } => exit_codes::CONFLICT,
            PicoError::Cancelled => exit_codes::CANCELLED,
            PicoError::Io(_) | PicoError::Internal { .. } => exit_codes::INTERNAL,
        }
    }

    /// Optional actionable hint explaining how to fix the problem.
    pub fn hint(&self) -> Option<String> {
        match self {
            PicoError::PathNotFound { path } => Some(format!(
                "check the spelling of the path: {}",
                path.display()
            )),
            PicoError::NotADirectory { .. } => Some(
                "raven-pico expects a project directory; pass a directory, not a file".to_string(),
            ),
            PicoError::ProjectNotDetected { .. } => Some(
                "supported markers: Cargo.toml, CMakeLists.txt, .sln/.slnx/.vcxproj, \
                 .csproj/.fsproj, pyproject.toml/setup.py/requirements.txt, project.godot, \
                 pom.xml, build.gradle(.kts), *.rockspec/main.lua, or raven-pico.toml; \
                 only the given directory is searched — pass --search-parents to also \
                 look in parent directories (see docs/toolchains.md)"
                    .to_string(),
            ),
            PicoError::AmbiguousProject { kinds, .. } => Some(format!(
                "pass --kind <KIND> (one of: {kinds}) or set [project] kind in raven-pico.toml"
            )),
            PicoError::ToolMissing { hint, .. } => Some(hint.clone()),
            PicoError::ToolFailed { .. } => Some(
                "inspect the captured stdout/stderr above; fix the project or the toolchain"
                    .to_string(),
            ),
            PicoError::UnsupportedOperation { .. } => Some(
                "configure the operation in raven-pico.toml (see docs/cli-reference.md) or use \
                 a supported operation"
                    .to_string(),
            ),
            PicoError::ConfigInvalid { .. } => Some(
                "fix raven-pico.toml; the schema is documented in docs/json-schema.md".to_string(),
            ),
            PicoError::ConfigExists { .. } => {
                Some("pass --force to overwrite the file explicitly".to_string())
            }
            PicoError::Cancelled => None,
            PicoError::Io(_) => None,
            PicoError::Internal { .. } => {
                Some("this is likely a bug in raven-pico; please report it".to_string())
            }
        }
    }

    /// Builds the JSON error payload used in reports.
    pub fn to_error_info(&self) -> ErrorInfo {
        ErrorInfo {
            code: self.code().as_str().to_string(),
            message: self.to_string(),
            hint: self.hint(),
        }
    }
}

use crate::report::ErrorInfo;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_codes_are_stable() {
        let missing = PicoError::ToolMissing {
            tool: "cargo".into(),
            hint: "install rust".into(),
        };
        assert_eq!(missing.exit_code(), exit_codes::TOOL_MISSING);
        assert_eq!(missing.code().as_str(), "tool_missing");
        assert!(missing.hint().is_some());
    }
}
