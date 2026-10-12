//! Raven Pico — the Raven ecosystem's standalone build backend and CLI.
//!
//! The binary is a thin shell over the reusable library crates:
//! `raven-projects` (detection) and `raven-build` (process execution and
//! adapters). All presentation lives in [`ui`]; all errors flow through
//! [`raven_core::error::PicoError`], which owns the documented exit codes.

mod cli;
mod ui;

use std::process::ExitCode;
use std::time::Instant;

use clap::Parser as _;
use raven_build::{CancelToken, Op, OpContext, Profile, StdioMode, execute_operation};
use raven_core::error::PicoError;
use raven_core::kind::ProjectKind;
use raven_core::path_to_string;
use raven_core::report::{
    DetectPayload, DoctorPayload, InitPayload, Report, ReportPayload, ToolchainsPayload,
};
use raven_projects::{DetectOptions, select};
use ui::{OutputMode, Reporter, TerminalSink};

fn main() -> ExitCode {
    let cli = cli::Cli::parse();
    let mode = OutputMode::from_flags(cli.json, cli.quiet, cli.color);
    let reporter = Reporter::new(mode);
    let sink = TerminalSink::new(mode);
    let started = Instant::now();

    let (report, code) = run_command(&cli, &sink, mode, started);
    reporter.emit(&report);
    ExitCode::from(code as u8)
}

/// Runs one subcommand and returns its report plus documented exit code.
fn run_command(
    cli: &cli::Cli,
    sink: &TerminalSink,
    mode: OutputMode,
    started: Instant,
) -> (Report, i32) {
    let version = env!("CARGO_PKG_VERSION");
    let cancel = CancelToken::new();

    let outcome: Result<Report, PicoError> = match &cli.command {
        cli::Command::Detect { path } => detect_command(path, cli.search_parents, version, started),
        cli::Command::Build {
            path,
            release,
            kind,
            tool_args,
        } => operation_command(
            Op::Build,
            path,
            *release,
            kind.as_deref(),
            tool_args,
            cli.search_parents,
            mode,
            sink,
            version,
            started,
        ),
        cli::Command::Test {
            path,
            release,
            kind,
            tool_args,
        } => operation_command(
            Op::Test,
            path,
            *release,
            kind.as_deref(),
            tool_args,
            cli.search_parents,
            mode,
            sink,
            version,
            started,
        ),
        cli::Command::Run {
            path,
            release,
            kind,
            tool_args,
        } => operation_command(
            Op::Run,
            path,
            *release,
            kind.as_deref(),
            tool_args,
            cli.search_parents,
            mode,
            sink,
            version,
            started,
        ),
        cli::Command::Clean {
            path,
            release,
            kind,
            tool_args,
        } => operation_command(
            Op::Clean,
            path,
            *release,
            kind.as_deref(),
            tool_args,
            cli.search_parents,
            mode,
            sink,
            version,
            started,
        ),
        cli::Command::Doctor => Ok(doctor_command(version, &cancel, started)),
        cli::Command::Toolchains => Ok(toolchains_command(version, &cancel, started)),
        cli::Command::Init { path, kind, force } => init_command(
            path,
            kind.as_deref(),
            *force,
            cli.search_parents,
            version,
            started,
        ),
    };

    match outcome {
        Ok(report) => {
            let code = report.exit_code;
            (report, code)
        }
        Err(error) => {
            let report = Report::new(command_name(&cli.command), version)
                .with_failure(error.exit_code(), error.to_error_info());
            (report, error.exit_code())
        }
    }
}

fn command_name(command: &cli::Command) -> &'static str {
    match command {
        cli::Command::Detect { .. } => "detect",
        cli::Command::Build { .. } => "build",
        cli::Command::Test { .. } => "test",
        cli::Command::Run { .. } => "run",
        cli::Command::Clean { .. } => "clean",
        cli::Command::Doctor => "doctor",
        cli::Command::Toolchains => "toolchains",
        cli::Command::Init { .. } => "init",
    }
}

fn detect_options(search_parents: bool) -> DetectOptions {
    DetectOptions {
        search_parents,
        ..DetectOptions::default()
    }
}

fn elapsed_ms(started: Instant) -> u64 {
    started.elapsed().as_millis() as u64
}

// ---------------------------------------------------------------- detect ----

fn detect_command(
    path: &std::path::Path,
    search_parents: bool,
    version: &str,
    started: Instant,
) -> Result<Report, PicoError> {
    match raven_projects::detect(path, &detect_options(search_parents)) {
        Ok(detection) => {
            let payload = detection.to_payload();
            let mut report =
                Report::new("detect", version).with_payload(ReportPayload::Detect(payload.clone()));
            report.duration_ms = Some(elapsed_ms(started));
            if payload.primary.is_some() {
                Ok(report.with_success(0))
            } else if payload.ambiguous {
                // Ambiguity is a distinct, documented outcome (exit 5).
                let error = PicoError::AmbiguousProject {
                    path: detection.root.clone(),
                    kinds: payload
                        .candidates
                        .iter()
                        .map(|candidate| candidate.kind.as_str())
                        .collect::<Vec<_>>()
                        .join(", "),
                };
                Ok(report.with_failure(error.exit_code(), error.to_error_info()))
            } else {
                // Detection ran but found nothing usable.
                let error = PicoError::ProjectNotDetected {
                    path: path.to_path_buf(),
                };
                Ok(report.with_failure(error.exit_code(), error.to_error_info()))
            }
        }
        Err(error) => {
            // Path-level failures still produce an informative payload.
            let payload = DetectPayload {
                searched: path_to_string(path),
                root: None,
                primary: None,
                candidates: Vec::new(),
                ambiguous: false,
                config: None,
                searched_upwards: false,
            };
            let mut report =
                Report::new("detect", version).with_payload(ReportPayload::Detect(payload));
            report.duration_ms = Some(elapsed_ms(started));
            Ok(report.with_failure(error.exit_code(), error.to_error_info()))
        }
    }
}

// ------------------------------------------------------------ build/test/run/clean ----

#[allow(clippy::too_many_arguments)]
fn operation_command(
    op: Op,
    path: &std::path::Path,
    release: bool,
    kind: Option<&str>,
    tool_args: &[String],
    search_parents: bool,
    mode: OutputMode,
    sink: &TerminalSink,
    version: &str,
    started: Instant,
) -> Result<Report, PicoError> {
    // 1. Validate the explicit kind (usage error, not detection error).
    let kind_override = match kind {
        Some(raw) => match ProjectKind::parse(raw) {
            Some(parsed) => Some(parsed),
            None => {
                return Err(PicoError::ConfigInvalid {
                    message: format!(
                        "unknown --kind '{raw}'; expected one of: {}",
                        ProjectKind::aliases()
                    ),
                });
            }
        },
        None => None,
    };

    // 2. Detect the project. Only the given directory is inspected unless
    //    --search-parents was passed (see docs/cli-reference.md).
    let detection = raven_projects::detect(path, &detect_options(search_parents))?;
    let (project_kind, root) = select(&detection, kind_override)?;

    // 3. Configuration (broken configs are hard errors via select/require).
    let config = match &detection.config {
        Some(loaded) => Some(loaded.require()?.clone()),
        None => None,
    };

    // 4. Run the operation. Interactive text-mode `run` inherits stdio so
    //    programs stay interactive; everything else is captured.
    let stdio = if op == Op::Run && !mode.json {
        StdioMode::Inherit
    } else {
        StdioMode::Capture
    };
    let ctx = OpContext {
        root: root.clone(),
        kind: project_kind,
        config: config.as_ref(),
        profile: if release {
            Profile::Release
        } else {
            Profile::Debug
        },
        extra_args: tool_args.to_vec(),
        env: config
            .as_ref()
            .map(|config| config.env.clone())
            .unwrap_or_default(),
        search_paths: raven_build::proc::path_dirs(),
        sink: Some(sink),
        cancel: CancelToken::new(),
        stdio,
    };

    let outcome = execute_operation(&ctx, op)?;
    let payload = outcome.to_payload(project_kind, ctx.profile, &root);
    let mut report = Report::new(op.as_str(), version).with_payload(match op {
        Op::Build => ReportPayload::Build(payload),
        Op::Test => ReportPayload::Test(payload),
        Op::Run => ReportPayload::Run(payload),
        Op::Clean => ReportPayload::Clean(payload),
    });
    report.duration_ms = Some(elapsed_ms(started));

    if outcome.success() {
        Ok(report.with_success(0))
    } else if outcome.cancelled() {
        let error = PicoError::Cancelled;
        Ok(report.with_failure(error.exit_code(), error.to_error_info()))
    } else {
        // The tool ran and failed; surface its exit code in the error block.
        let tool_exit = outcome.result.as_ref().and_then(|result| result.exit_code);
        let error = PicoError::ToolFailed {
            tool: outcome
                .tool
                .as_ref()
                .map(|tool| tool.name.clone())
                .unwrap_or_else(|| "command".to_string()),
            exit_code: tool_exit.unwrap_or(-1),
        };
        Ok(report.with_failure(1, error.to_error_info()))
    }
}

// ---------------------------------------------------------- doctor/toolchains ----

fn doctor_command(version: &str, cancel: &CancelToken, started: Instant) -> Report {
    let checks = raven_build::doctor::doctor_checks(cancel);
    let healthy = raven_build::doctor::is_healthy(&checks);
    let payload = DoctorPayload {
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        healthy,
        checks: raven_build::doctor::to_dtos(&checks),
    };
    let mut report = Report::new("doctor", version).with_payload(ReportPayload::Doctor(payload));
    report.duration_ms = Some(elapsed_ms(started));
    report.with_success(0)
}

fn toolchains_command(version: &str, cancel: &CancelToken, started: Instant) -> Report {
    let checks = raven_build::doctor::collect_tool_checks(cancel);
    let found = checks.iter().filter(|check| check.status == "ok").count();
    let missing = checks
        .iter()
        .filter(|check| check.status == "missing")
        .count();
    let payload = ToolchainsPayload {
        checks: raven_build::doctor::to_dtos(&checks),
        found,
        missing,
    };
    let mut report =
        Report::new("toolchains", version).with_payload(ReportPayload::Toolchains(payload));
    report.duration_ms = Some(elapsed_ms(started));
    report.with_success(0)
}

// -------------------------------------------------------------------- init ----

fn init_command(
    path: &std::path::Path,
    kind: Option<&str>,
    force: bool,
    search_parents: bool,
    version: &str,
    started: Instant,
) -> Result<Report, PicoError> {
    // Validate the kind early: never write a file we would reject later.
    let explicit_kind = match kind {
        Some(raw) => Some(
            ProjectKind::parse(raw).ok_or_else(|| PicoError::ConfigInvalid {
                message: format!(
                    "unknown --kind '{raw}'; expected one of: {}",
                    ProjectKind::aliases()
                ),
            })?,
        ),
        None => None,
    };

    let dir = raven_projects::validate_directory(path)?;
    let config_path = dir.join(raven_core::config::CONFIG_FILE_NAME);
    if config_path.exists() && !force {
        return Err(PicoError::ConfigExists { path: config_path });
    }

    // Pre-select a kind: explicit flag first, then detection.
    let detected_kind = explicit_kind.or_else(|| {
        raven_projects::detect(&dir, &detect_options(search_parents))
            .ok()
            .and_then(|detection| detection.primary)
            .map(|candidate| candidate.kind)
    });

    let template = raven_core::config::template(detected_kind);
    std::fs::write(&config_path, template)?;

    let payload = InitPayload {
        config_path: path_to_string(&config_path),
        created: true,
        kind: detected_kind.map(|kind| kind.as_str().to_string()),
    };
    let mut report = Report::new("init", version).with_payload(ReportPayload::Init(payload));
    report.duration_ms = Some(elapsed_ms(started));
    Ok(report.with_success(0))
}
