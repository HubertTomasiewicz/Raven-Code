//! # raven-build
//!
//! Build-system adapters, process execution, toolchain discovery, testing,
//! running and diagnostics for Raven Pico.
//!
//! Layers:
//!
//! * [`proc`] — the reusable process-execution layer (capture, cancellation,
//!   timeouts, bounded memory, structured launch errors).
//! * [`toolchain`] — discovery of installed tools with version probing.
//! * adapters — one module per build system implementing
//!   [`BuildAdapter`].
//!
//! Command selection precedence for every operation:
//!
//! 1. a `[build]` / `[test]` / `[run]` / `[clean]` section in
//!    `raven-pico.toml` (project-defined command),
//! 2. the native adapter for the project kind,
//! 3. otherwise a structured "unsupported" error — Pico never claims support
//!    for anything that is not implemented.
//!
//! The adapter API is deliberately future-proof: it receives a
//! [`CancelToken`] and an optional [`ProgressSink`], so a future Raven
//! service can stream progress and cancel operations without changing this
//! layer.

pub mod doctor;
pub mod proc;
pub mod toolchain;

pub mod adapters;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use raven_core::config::CmdSpec;
use raven_core::error::PicoError;
use raven_core::kind::ProjectKind;
use raven_core::path_to_string;
use raven_core::progress::{ProgressEvent, ProgressSink};
use raven_core::report::{OpPayload, ToolInfoDto};

pub use proc::{CancelToken, ProcessResult, ProcessSpec, StdioMode};
pub use toolchain::{TOOL_CATALOG, ToolSpec, probe_tool};

/// Operations supported on a project.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Op {
    Build,
    Test,
    Run,
    Clean,
}

impl Op {
    pub fn as_str(self) -> &'static str {
        match self {
            Op::Build => "build",
            Op::Test => "test",
            Op::Run => "run",
            Op::Clean => "clean",
        }
    }

    /// The `raven-pico.toml` section that overrides this operation.
    pub fn config_section(self) -> &'static str {
        self.as_str()
    }

    pub fn all() -> [Op; 4] {
        [Op::Build, Op::Test, Op::Run, Op::Clean]
    }
}

/// Build profile requested on the CLI (`--release`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    Debug,
    Release,
}

impl Profile {
    pub fn as_str(self) -> &'static str {
        match self {
            Profile::Debug => "debug",
            Profile::Release => "release",
        }
    }
}

/// Whether an adapter supports an operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Support {
    /// Natively implemented.
    Available,
    /// Only when configured in `raven-pico.toml`.
    ConfigRequired { hint: Option<String> },
    /// Not supported at all.
    Unsupported {
        reason: String,
        hint: Option<String>,
    },
}

/// Capability matrix of an adapter.
#[derive(Debug, Clone)]
pub struct Capabilities {
    pub build: Support,
    pub test: Support,
    pub run: Support,
    pub clean: Support,
}

impl Capabilities {
    pub fn support(&self, op: Op) -> &Support {
        match op {
            Op::Build => &self.build,
            Op::Test => &self.test,
            Op::Run => &self.run,
            Op::Clean => &self.clean,
        }
    }
}

/// Everything an adapter needs to perform an operation.
pub struct OpContext<'a> {
    /// Absolute (or user-provided) project root.
    pub root: PathBuf,
    pub kind: ProjectKind,
    /// Parsed `raven-pico.toml`, when present and valid.
    pub config: Option<&'a raven_core::config::PicoConfig>,
    pub profile: Profile,
    /// Extra arguments appended to the tool invocation (`--tool-arg`).
    pub extra_args: Vec<String>,
    /// Environment additions for spawned processes (from config `[env]`).
    pub env: BTreeMap<String, String>,
    /// Search paths used to resolve tools; empty means the process PATH.
    pub search_paths: Vec<PathBuf>,
    pub sink: Option<&'a dyn ProgressSink>,
    pub cancel: CancelToken,
    /// Whether output should be captured (JSON mode) or inherited
    /// (interactive text-mode `run`).
    pub stdio: StdioMode,
}

impl<'a> OpContext<'a> {
    /// Emits a progress event to the sink, if any.
    pub fn emit(&self, event: ProgressEvent) {
        if let Some(sink) = self.sink {
            sink.on_event(&event);
        }
    }

    fn stage(&self, message: impl Into<String>) {
        self.emit(ProgressEvent::Stage {
            message: message.into(),
        });
    }
}

/// The result of an operation performed by an adapter or configuration.
#[derive(Debug, Clone)]
pub struct OperationOutcome {
    /// The tool that was invoked, when a tool ran.
    pub tool: Option<ToolInfoDto>,
    /// Exact argv executed.
    pub command: Option<Vec<String>>,
    /// Working directory used.
    pub cwd: Option<PathBuf>,
    /// Raw process result. `None` for built-in operations that did not spawn
    /// a process (e.g. removing Godot's `.godot` cache).
    pub result: Option<ProcessResult>,
    /// Generated artifacts known to Pico.
    pub artifacts: Vec<PathBuf>,
    /// Additional human-readable note.
    pub note: Option<String>,
}

impl OperationOutcome {
    pub fn success(&self) -> bool {
        match &self.result {
            Some(result) => result.success(),
            None => true,
        }
    }

    pub fn cancelled(&self) -> bool {
        self.result.as_ref().is_some_and(|result| result.cancelled)
    }

    pub fn duration(&self) -> Duration {
        self.result
            .as_ref()
            .map(|result| result.duration)
            .unwrap_or(Duration::ZERO)
    }

    /// Converts the outcome into the versioned JSON payload.
    pub fn to_payload(
        &self,
        kind: ProjectKind,
        profile: Profile,
        root: &std::path::Path,
    ) -> OpPayload {
        let (stdout, stderr, exit_code, duration_ms, cancelled, timed_out, truncated) =
            match &self.result {
                Some(result) => (
                    match result.stdout.is_empty() {
                        true => None,
                        false => Some(result.stdout.clone()),
                    },
                    match result.stderr.is_empty() {
                        true => None,
                        false => Some(result.stderr.clone()),
                    },
                    result.exit_code,
                    Some(result.duration.as_millis() as u64),
                    result.cancelled,
                    result.timed_out,
                    result.truncated,
                ),
                None => (None, None, None, None, false, false, false),
            };
        OpPayload {
            kind: kind.as_str().to_string(),
            root: path_to_string(root),
            profile: profile.as_str().to_string(),
            tool: self.tool.clone(),
            command: self.command.clone(),
            cwd: self.cwd.as_ref().map(|cwd| path_to_string(cwd)),
            exit_code,
            duration_ms,
            stdout,
            stderr,
            artifacts: self.artifacts.iter().map(|a| path_to_string(a)).collect(),
            cancelled,
            timed_out,
            truncated,
            note: self.note.clone(),
        }
    }
}

/// A build-system adapter for one project kind.
pub trait BuildAdapter: Send + Sync {
    fn kind(&self) -> ProjectKind;

    /// Which operations this adapter natively supports for the project at
    /// `ctx.root` (configuration-independent; configuration is handled by
    /// [`execute_operation`]).
    fn capabilities(&self, ctx: &OpContext) -> Capabilities;

    fn build(&self, ctx: &OpContext) -> Result<OperationOutcome, PicoError>;
    fn test(&self, ctx: &OpContext) -> Result<OperationOutcome, PicoError>;
    fn run(&self, ctx: &OpContext) -> Result<OperationOutcome, PicoError>;
    fn clean(&self, ctx: &OpContext) -> Result<OperationOutcome, PicoError>;
}

/// Adapters, ordered by [`ProjectKind::all`].
pub fn all_adapters() -> Vec<&'static dyn BuildAdapter> {
    vec![
        &adapters::cargo::CargoAdapter,
        &adapters::cmake::CmakeAdapter,
        &adapters::msbuild::MsBuildAdapter,
        &adapters::dotnet::DotNetAdapter,
        &adapters::python::PythonAdapter,
        &adapters::godot::GodotAdapter,
        &adapters::maven::MavenAdapter,
        &adapters::gradle::GradleAdapter,
        &adapters::lua::LuaAdapter,
        &adapters::custom::CustomAdapter,
    ]
}

/// Returns the adapter for a project kind.
pub fn adapter_for(kind: ProjectKind) -> Option<&'static dyn BuildAdapter> {
    all_adapters()
        .into_iter()
        .find(|adapter| adapter.kind() == kind)
}

/// Resolves a tool, producing an actionable [`PicoError::ToolMissing`] when
/// it cannot be found.
pub fn resolve_tool(ctx: &OpContext, name: &str, hint: &str) -> Result<PathBuf, PicoError> {
    let dirs = if ctx.search_paths.is_empty() {
        proc::path_dirs()
    } else {
        ctx.search_paths.clone()
    };
    proc::resolve_in(&dirs, name).ok_or_else(|| PicoError::ToolMissing {
        tool: name.to_string(),
        hint: hint.to_string(),
    })
}

/// Runs a configured command from `raven-pico.toml`.
///
/// The command is executed directly — no shell — with the project root as
/// working directory (or the configured relative `cwd`). This executes
/// project code by design; callers must have made that clear to the user.
pub fn run_config_command(
    ctx: &OpContext,
    section: &CmdSpec,
) -> Result<OperationOutcome, PicoError> {
    let mut spec = ProcessSpec::new(section.command.clone());
    spec.args = section.args.clone();
    spec.args.extend(ctx.extra_args.iter().cloned());
    match &section.cwd {
        Some(cwd) => spec.cwd = Some(ctx.root.join(cwd)),
        None => spec.cwd = Some(ctx.root.clone()),
    }
    spec.env = ctx.env.clone();
    spec.stdio = ctx.stdio;
    spec.search_paths = ctx.search_paths.clone();
    execute_spec(ctx, None, spec, Vec::new(), None)
}

/// Executes a [`ProcessSpec`], reporting progress and packaging the result.
///
/// `tool` (name/version/path) is attached to the outcome when provided.
pub fn execute_spec(
    ctx: &OpContext,
    tool: Option<ToolInfoDto>,
    spec: ProcessSpec,
    artifacts: Vec<PathBuf>,
    note: Option<String>,
) -> Result<OperationOutcome, PicoError> {
    ctx.emit(ProgressEvent::CommandSpawned {
        program: spec.program.clone(),
        args: spec.args.clone(),
    });
    let result = proc::run_process(&spec, &ctx.cancel);
    ctx.emit(ProgressEvent::CommandFinished {
        exit_code: result.exit_code,
        duration_ms: result.duration.as_millis() as u64,
    });
    Ok(OperationOutcome {
        tool,
        command: Some(spec.command_line()),
        cwd: spec.cwd.clone(),
        result: Some(result),
        artifacts,
        note,
    })
}

/// Plans and executes one operation for a project.
///
/// Precedence: configuration section, then native adapter, then a structured
/// unsupported error.
pub fn execute_operation(ctx: &OpContext, op: Op) -> Result<OperationOutcome, PicoError> {
    ctx.stage(format!(
        "{}: {}",
        op.as_str(),
        ctx.kind.display_name().to_lowercase()
    ));

    // 1. Project-defined command from raven-pico.toml takes precedence.
    if let Some(config) = ctx.config {
        let section = match op {
            Op::Build => config.build.as_ref(),
            Op::Test => config.test.as_ref(),
            Op::Run => config.run.as_ref(),
            Op::Clean => config.clean.as_ref(),
        };
        if let Some(section) = section {
            if ctx.profile == Profile::Release {
                return Err(PicoError::UnsupportedOperation {
                    operation: op.as_str().to_string(),
                    kind: ctx.kind.display_name().to_string(),
                    reason: "configured commands in raven-pico.toml do not take --release"
                        .to_string(),
                });
            }
            return run_config_command(ctx, section);
        }
    }

    // 2. Native adapter.
    let adapter = adapter_for(ctx.kind).ok_or_else(|| PicoError::Internal {
        message: format!("no adapter registered for {}", ctx.kind.as_str()),
    })?;

    let support = adapter.capabilities(ctx).support(op).clone();
    match support {
        Support::Available => match op {
            Op::Build => adapter.build(ctx),
            Op::Test => adapter.test(ctx),
            Op::Run => adapter.run(ctx),
            Op::Clean => adapter.clean(ctx),
        },
        Support::ConfigRequired { hint } => Err(PicoError::UnsupportedOperation {
            operation: op.as_str().to_string(),
            kind: ctx.kind.display_name().to_string(),
            reason: match hint {
                Some(hint) => {
                    format!("requires a project-defined command in raven-pico.toml ({hint})")
                }
                None => "requires a project-defined command in raven-pico.toml".to_string(),
            },
        }),
        Support::Unsupported { reason, .. } => Err(PicoError::UnsupportedOperation {
            operation: op.as_str().to_string(),
            kind: ctx.kind.display_name().to_string(),
            reason,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use raven_core::progress::SilentSink;

    #[test]
    fn every_kind_has_an_adapter() {
        for kind in ProjectKind::all() {
            assert!(adapter_for(*kind).is_some(), "missing adapter for {kind:?}");
        }
    }

    #[test]
    fn operation_names_are_stable() {
        assert_eq!(Op::Build.as_str(), "build");
        assert_eq!(Op::Clean.as_str(), "clean");
        assert_eq!(Profile::Debug.as_str(), "debug");
    }

    #[test]
    fn config_precedence_over_native_adapter() {
        // A config-defined command must run even for kinds with a native
        // adapter (cargo project with a [build] override).
        let dir = tempfile::tempdir().unwrap();
        let config_text = "schema_version = 1\n[build]\ncommand = \"make\"\nargs = [\"all\"]\n";
        let config = raven_core::config::PicoConfig::parse(config_text).unwrap();
        let tool = testkit::FakeTool::new("make");
        let log_env = tool.log_env();

        let ctx = OpContext {
            root: dir.path().to_path_buf(),
            kind: ProjectKind::Rust,
            config: Some(&config),
            profile: Profile::Debug,
            extra_args: Vec::new(),
            env: [(log_env.0.clone(), log_env.1.clone())]
                .into_iter()
                .collect(),
            search_paths: tool.search_paths(),
            sink: Some(&SilentSink),
            cancel: CancelToken::new(),
            stdio: StdioMode::Capture,
        };
        let outcome = execute_operation(&ctx, Op::Build).unwrap();
        assert!(outcome.success(), "outcome: {outcome:?}");
        assert_eq!(tool.log_lines(), vec!["fake make all"]);
        assert_eq!(
            outcome.command.as_deref(),
            Some(&["make".to_string(), "all".to_string()][..])
        );
    }
}
