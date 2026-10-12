//! Build-system adapters, one per supported ecosystem.
//!
//! Adding an adapter: create a module here implementing
//! [`BuildAdapter`](crate::BuildAdapter), register it in
//! [`all_adapters`](crate::all_adapters), and add its detection markers in
//! `raven-projects`. The procedure is documented in `docs/adding-adapters.md`.

pub mod cargo;
pub mod cmake;
pub mod custom;
pub mod dotnet;
pub mod godot;
pub mod gradle;
pub mod lua;
pub mod maven;
pub mod msbuild;
pub mod python;

/// Shared helpers for adapters.
pub(crate) mod support {
    use std::path::PathBuf;

    use raven_core::report::ToolInfoDto;

    use crate::proc::ProcessSpec;
    use crate::{OpContext, execute_spec, resolve_tool};

    /// Resolves a tool on PATH (or the context search paths) and returns the
    /// resolved path plus its cheap version probe.
    pub(crate) fn resolve(
        ctx: &OpContext,
        name: &str,
        version_args: &[&str],
        hint: &str,
    ) -> Result<(PathBuf, ToolInfoDto), raven_core::error::PicoError> {
        let path = resolve_tool(ctx, name, hint)?;
        let mut info = ToolInfoDto {
            name: name.to_string(),
            version: None,
            // Serialized through the shared path rules (no verbatim prefix).
            path: Some(raven_core::path_to_string(&path)),
        };
        // Version probing is best-effort: a tool that refuses --version is
        // still usable.
        let cancel = ctx.cancel.clone();
        let mut spec = ProcessSpec::new(path.display().to_string());
        for arg in version_args {
            spec = spec.arg(*arg);
        }
        let spec = spec.timeout(std::time::Duration::from_secs(4));
        let result = crate::proc::run_process(&spec, &cancel);
        if result.success() {
            let combined = format!("{}\n{}", result.stdout, result.stderr);
            let version = crate::toolchain::parse_version(name, &combined);
            if !version.is_empty() {
                info.version = Some(version);
            }
        }
        ctx.emit(raven_core::progress::ProgressEvent::ToolResolved {
            tool: name.to_string(),
            version: info.version.clone(),
        });
        Ok((path, info))
    }

    /// Executes a fully built spec with the context's stdio/env policy.
    pub(crate) fn execute(
        ctx: &OpContext,
        tool: ToolInfoDto,
        mut spec: ProcessSpec,
        artifacts: Vec<PathBuf>,
        note: Option<String>,
    ) -> Result<crate::OperationOutcome, raven_core::error::PicoError> {
        spec.env.extend(ctx.env.clone());
        spec.stdio = ctx.stdio;
        execute_spec(ctx, Some(tool), spec, artifacts, note)
    }

    /// Applies `--tool-arg` extras to a spec.
    pub(crate) fn with_extra_args(spec: ProcessSpec, ctx: &OpContext) -> ProcessSpec {
        if ctx.extra_args.is_empty() {
            spec
        } else {
            spec.args(ctx.extra_args.as_slice())
        }
    }
}
