//! The versioned JSON report envelope (schema version 1).
//!
//! Every `raven-pico` command supports `--json`. In that mode the command
//! prints **exactly one** JSON document on stdout (pretty-printed), and any
//! human-readable progress goes to stderr. The envelope is stable:
//! `schema_version = 1` will not change incompatibly.

use crate::error::exit_codes;

/// JSON report schema version. Part of the compatibility contract.
pub const REPORT_SCHEMA_VERSION: u64 = 1;

/// The tool name written into every report.
pub const TOOL_NAME: &str = "raven-pico";

/// A detected or resolved tool (name, version, path).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ToolInfoDto {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

/// A project candidate produced by detection.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CandidateDto {
    /// Canonical kind id, e.g. `"rust"`.
    pub kind: String,
    /// Human-readable display name, e.g. `"Rust (Cargo)"`.
    pub display: String,
    /// `explicit` | `high` | `medium`.
    pub confidence: String,
    /// Marker files that matched.
    pub markers: Vec<String>,
    /// Directory where the candidate was found.
    pub root: String,
}

/// Status of the optional `raven-pico.toml` in the detected project.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ConfigDto {
    pub path: String,
    /// `ok` | `parse_error` | `unsupported_version`.
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema_version: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind_override: Option<String>,
    /// Operation sections defined in the file (`build`, `test`, `run`, `clean`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub defined_sections: Vec<String>,
}

/// Payload of the `detect` command.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DetectPayload {
    /// Directory that was searched.
    pub searched: String,
    /// Project root that was selected (may be an ancestor of `searched`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root: Option<String>,
    /// The unambiguous primary candidate, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary: Option<CandidateDto>,
    /// All detected candidates, ranked.
    pub candidates: Vec<CandidateDto>,
    /// True when two or more candidates tie for the top rank and an explicit
    /// choice (`--kind` or `raven-pico.toml`) is required.
    pub ambiguous: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config: Option<ConfigDto>,
    /// True when the root was found by searching parent directories.
    pub searched_upwards: bool,
}

/// Payload of the `build`, `test`, `run` and `clean` commands.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct OpPayload {
    /// Canonical project kind id.
    pub kind: String,
    /// Project root (absolute).
    pub root: String,
    /// `debug` | `release` | `n/a`.
    pub profile: String,
    /// The tool that was invoked (absent for built-in operations like
    /// removing a cache directory).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<ToolInfoDto>,
    /// Exact argv that was executed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<Vec<String>>,
    /// Working directory for the invocation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// Exit code of the invoked tool. `null` when the tool could not be
    /// launched, was killed, or no tool ran.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    /// Captured stdout. Always `null` when stdio was inherited (text-mode
    /// `run`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stdout: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stderr: Option<String>,
    /// Generated artifact paths known to Pico.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<String>,
    pub cancelled: bool,
    pub timed_out: bool,
    /// True when captured output exceeded the capture limit and was cut off.
    pub truncated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// A single environment check reported by `doctor` / `toolchains`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CheckDto {
    /// Tool id, e.g. `"cargo"`.
    pub id: String,
    /// `ok` | `missing` | `error`.
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

/// Payload of the `doctor` command.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DoctorPayload {
    /// Operating system, e.g. `"windows"`.
    pub os: String,
    /// CPU architecture, e.g. `"x86_64"`.
    pub arch: String,
    /// True when no check has status `error` and at least one toolchain is
    /// usable. `doctor` still exits 0 — it is a report, not an assertion.
    pub healthy: bool,
    pub checks: Vec<CheckDto>,
}

/// Payload of the `toolchains` command.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ToolchainsPayload {
    pub checks: Vec<CheckDto>,
    pub found: usize,
    pub missing: usize,
}

/// Payload of the `init` command.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct InitPayload {
    pub config_path: String,
    pub created: bool,
    /// Kind pre-selected in the generated template, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

/// Structured, actionable error information.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ErrorInfo {
    /// Stable machine-readable code, e.g. `"tool_missing"`.
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

/// The report envelope written by every command in `--json` mode.
///
/// The payload for a command appears under the key named after the command
/// (`detect`, `build`, `test`, `run`, `clean`, `doctor`, `toolchains`, `init`);
/// all other payload keys are omitted.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Report {
    pub schema_version: u64,
    pub tool: &'static str,
    pub tool_version: String,
    /// The command that ran: `detect`, `build`, `test`, `run`, `clean`,
    /// `doctor`, `toolchains` or `init`.
    pub command: String,
    /// True when the operation achieved its goal (tool exit code 0, detection
    /// produced a usable project, ...).
    pub success: bool,
    /// Raven Pico's own documented exit code — NOT the exit code of the tool
    /// that was invoked (that lives in the payload as `exit_code`).
    pub exit_code: i32,
    /// Wall-clock duration of the whole command, in milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detect: Option<DetectPayload>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build: Option<OpPayload>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub test: Option<OpPayload>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run: Option<OpPayload>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clean: Option<OpPayload>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub doctor: Option<DoctorPayload>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub toolchains: Option<ToolchainsPayload>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub init: Option<InitPayload>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorInfo>,
}

impl Report {
    /// Starts a new report for the given command.
    pub fn new(command: &str, tool_version: &str) -> Report {
        Report {
            schema_version: REPORT_SCHEMA_VERSION,
            tool: TOOL_NAME,
            tool_version: tool_version.to_string(),
            command: command.to_string(),
            success: false,
            exit_code: exit_codes::FAILED,
            duration_ms: None,
            detect: None,
            build: None,
            test: None,
            run: None,
            clean: None,
            doctor: None,
            toolchains: None,
            init: None,
            error: None,
        }
    }

    /// Marks the report as successful with the given exit code.
    pub fn with_success(mut self, exit_code: i32) -> Report {
        self.success = true;
        self.exit_code = exit_code;
        self
    }

    /// Marks the report as failed with the given exit code and error info.
    pub fn with_failure(mut self, exit_code: i32, error: ErrorInfo) -> Report {
        self.success = false;
        self.exit_code = exit_code;
        self.error = Some(error);
        self
    }

    /// Attaches a payload under the key matching `command`.
    pub fn with_payload(self, payload: ReportPayload) -> Report {
        let mut report = self;
        match payload {
            ReportPayload::Detect(payload) => report.detect = Some(payload),
            ReportPayload::Build(payload) => report.build = Some(payload),
            ReportPayload::Test(payload) => report.test = Some(payload),
            ReportPayload::Run(payload) => report.run = Some(payload),
            ReportPayload::Clean(payload) => report.clean = Some(payload),
            ReportPayload::Doctor(payload) => report.doctor = Some(payload),
            ReportPayload::Toolchains(payload) => report.toolchains = Some(payload),
            ReportPayload::Init(payload) => report.init = Some(payload),
        }
        report
    }
}

/// Payload variants that can be attached to a report.
#[derive(Debug, Clone)]
pub enum ReportPayload {
    Detect(DetectPayload),
    Build(OpPayload),
    Test(OpPayload),
    Run(OpPayload),
    Clean(OpPayload),
    Doctor(DoctorPayload),
    Toolchains(ToolchainsPayload),
    Init(InitPayload),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kind::Confidence;

    #[test]
    fn envelope_is_valid_json_with_stable_shape() {
        let report = Report::new("detect", "0.1.0")
            .with_success(exit_codes::SUCCESS)
            .with_payload(ReportPayload::Detect(DetectPayload {
                searched: "/tmp/x".into(),
                root: Some("/tmp/x".into()),
                primary: Some(CandidateDto {
                    kind: "rust".into(),
                    display: "Rust (Cargo)".into(),
                    confidence: Confidence::High.as_str().into(),
                    markers: vec!["Cargo.toml".into()],
                    root: "/tmp/x".into(),
                }),
                candidates: vec![],
                ambiguous: false,
                config: None,
                searched_upwards: false,
            }));

        let value: serde_json::Value = serde_json::to_value(&report).unwrap();
        assert_eq!(value["schema_version"], 1);
        assert_eq!(value["tool"], "raven-pico");
        assert_eq!(value["command"], "detect");
        assert_eq!(value["success"], true);
        assert_eq!(value["exit_code"], 0);
        assert_eq!(value["detect"]["primary"]["kind"], "rust");
        assert!(
            value.get("build").is_none(),
            "unused payload keys must be omitted"
        );
    }

    #[test]
    fn error_payload_carries_code_and_hint() {
        let report = Report::new("build", "0.1.0").with_failure(
            exit_codes::TOOL_MISSING,
            ErrorInfo {
                code: "tool_missing".into(),
                message: "required tool 'cargo' was not found".into(),
                hint: Some("install rust via rustup".into()),
            },
        );
        let value: serde_json::Value = serde_json::to_value(&report).unwrap();
        assert_eq!(value["success"], false);
        assert_eq!(value["exit_code"], 4);
        assert_eq!(value["error"]["code"], "tool_missing");
        assert_eq!(value["error"]["hint"], "install rust via rustup");
    }
}
