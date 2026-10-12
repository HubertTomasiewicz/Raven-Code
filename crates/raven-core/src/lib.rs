//! # raven-core
//!
//! Shared foundation for Raven Pico and future Raven applications
//! (Raven Studio, Raven Code, Raven Hub).
//!
//! This crate deliberately contains **no I/O beyond configuration loading**
//! and no process execution. It provides:
//!
//! * [`error::PicoError`] — the single error type used across the ecosystem,
//!   with stable machine-readable codes and documented process exit codes.
//! * [`kind::ProjectKind`] — the ecosystems Pico understands.
//! * [`paths`] — the single path display/serialization rule set (Windows
//!   verbatim `\\?\` prefixes are cleaned for humans and JSON, safely).
//! * [`config::PicoConfig`] — the versioned `raven-pico.toml` project
//!   configuration file (schema version 1).
//! * [`report::Report`] — the versioned JSON report envelope (schema version 1)
//!   emitted by every command in `--json` mode.
//! * [`progress::ProgressEvent`] / [`progress::ProgressSink`] — the seam future
//!   Raven applications and services plug into for progress streaming.
//!
//! Stability contract: the JSON report and configuration schemas are versioned.
//! Schema version 1 will not change incompatibly; additions happen under a new
//! schema version.

pub mod config;
pub mod error;
pub mod kind;
pub mod paths;
pub mod progress;
pub mod report;

pub use config::{
    CONFIG_FILE_NAME, CmakeSection, CmdSpec, ConfigStatus, LoadedConfig, PicoConfig,
    ProjectSection, SCHEMA_VERSION,
};
pub use error::{ErrorCode, PicoError, exit_codes};
pub use kind::{Confidence, ProjectKind};
pub use paths::{clean_verbatim, display_path};
pub use progress::{ProgressEvent, ProgressSink, SilentSink};
pub use report::{
    CandidateDto, CheckDto, ConfigDto, DetectPayload, DoctorPayload, ErrorInfo, InitPayload,
    OpPayload, REPORT_SCHEMA_VERSION, Report, TOOL_NAME, ToolInfoDto, ToolchainsPayload,
};

/// Human-readable tool identity used in reports and UI.
pub const APP_DISPLAY_NAME: &str = "Raven Pico";

/// Converts a path to a printable string (lossy on non-UTF-8 platforms).
///
/// This is the one path serialization used for human output and JSON
/// reports: Windows verbatim `\\?\` prefixes are removed when — and only
/// when — that cannot change what the path points at (see [`paths`]).
pub fn path_to_string(path: &std::path::Path) -> String {
    paths::display_path(path)
}
