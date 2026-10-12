//! Adapter for Visual Studio solutions (`.sln`, `.slnx`) and native MSBuild
//! projects (`.vcxproj`).
//!
//! Tool discovery order:
//!
//! 1. `MSBuild.exe` located via vswhere (the supported mechanism on Windows),
//! 2. a fallback scan of `Program Files\Microsoft Visual Studio\...`,
//! 3. `dotnet msbuild` when the .NET SDK is installed (works for
//!    SDK-style solutions; on non-Windows this is the only route).
//!
//! `run` and `test` have no universal native meaning for MSBuild solutions;
//! they require a project-defined command (`[run]` / `[test]` in
//! `raven-pico.toml`).

use std::path::{Path, PathBuf};

use raven_core::error::PicoError;
use raven_core::kind::ProjectKind;
use raven_core::report::ToolInfoDto;

use crate::proc::ProcessSpec;
use crate::toolchain::find_msbuild;
use crate::{Capabilities, OpContext, OperationOutcome, Profile, Support};

use super::support;

pub struct MsBuildAdapter;

impl MsBuildAdapter {
    /// Finds the first solution / project file in the root (sorted, no
    /// recursion).
    fn project_file(root: &Path) -> Option<PathBuf> {
        let mut files: Vec<PathBuf> = std::fs::read_dir(root)
            .ok()?
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                matches!(
                    path.extension().and_then(|ext| ext.to_str()),
                    Some("sln") | Some("slnx") | Some("vcxproj")
                )
            })
            .collect();
        files.sort();
        // Prefer a solution over a bare project when both exist.
        files.sort_by_key(|path| {
            let is_sln = matches!(
                path.extension().and_then(|ext| ext.to_str()),
                Some("sln") | Some("slnx")
            );
            if is_sln { 0 } else { 1 }
        });
        files.into_iter().next()
    }

    /// Resolves an MSBuild-capable tool. Returns the program plus the
    /// arguments prefix (empty for MSBuild.exe; `["msbuild"]` for dotnet).
    fn resolve_tool(&self, ctx: &OpContext) -> Result<(Vec<String>, ToolInfoDto), PicoError> {
        if let Some(msbuild) = find_msbuild() {
            let mut info = ToolInfoDto {
                name: "msbuild".to_string(),
                version: None,
                path: Some(msbuild.display().to_string()),
            };
            // Best-effort version probe; -version output is multi-line.
            let spec = ProcessSpec::new(msbuild.display().to_string())
                .arg("-version")
                .arg("-nologo")
                .timeout(std::time::Duration::from_secs(6));
            let result = crate::proc::run_process(&spec, &ctx.cancel);
            if result.success() {
                let combined = format!("{}\n{}", result.stdout, result.stderr);
                info.version = combined
                    .lines()
                    .map(str::trim)
                    .rev()
                    .find(|line| !line.is_empty())
                    .map(str::to_string);
            }
            return Ok((Vec::new(), info));
        }
        // Fallback: dotnet msbuild handles SDK-style solutions.
        let dotnet = support::resolve(
            ctx,
            "dotnet",
            &["--version"],
            "install the .NET SDK: https://dotnet.microsoft.com/download",
        )?;
        Ok((
            vec!["msbuild".to_string()],
            ToolInfoDto {
                name: "dotnet".to_string(),
                version: dotnet.1.version,
                path: dotnet.1.path,
            },
        ))
    }

    fn msbuild_spec(
        &self,
        ctx: &OpContext,
        prefix: &[String],
        tool_path: &Path,
        target: &[&str],
    ) -> ProcessSpec {
        let mut spec = ProcessSpec::new(tool_path.display().to_string());
        for arg in prefix {
            spec = spec.arg(arg.clone());
        }
        if let Some(project) = Self::project_file(&ctx.root) {
            spec = spec.arg(project.display().to_string());
        }
        for arg in target {
            spec = spec.arg(*arg);
        }
        let configuration = match ctx.profile {
            Profile::Debug => "Debug",
            Profile::Release => "Release",
        };
        spec = spec
            .arg("-p:Configuration=".to_string() + configuration)
            .arg("-m")
            .arg("-nologo");
        spec.cwd(ctx.root.clone())
    }
}

impl crate::BuildAdapter for MsBuildAdapter {
    fn kind(&self) -> ProjectKind {
        ProjectKind::VisualStudio
    }

    fn capabilities(&self, _ctx: &OpContext) -> Capabilities {
        Capabilities {
            build: Support::Available,
            test: Support::ConfigRequired {
                hint: Some(
                    "Pico does not integrate vstest yet; set [test] in raven-pico.toml".to_string(),
                ),
            },
            run: Support::ConfigRequired {
                hint: Some("set [run] command to your built executable".to_string()),
            },
            clean: Support::Available,
        }
    }

    fn build(&self, ctx: &OpContext) -> Result<OperationOutcome, PicoError> {
        let (prefix, tool) = self.resolve_tool(ctx)?;
        let tool_path = PathBuf::from(tool.path.clone().unwrap_or_default());
        let mut spec = self.msbuild_spec(ctx, &prefix, &tool_path, &[]);
        spec = support::with_extra_args(spec, ctx);
        support::execute(ctx, tool, spec, Vec::new(), None)
    }

    fn test(&self, _ctx: &OpContext) -> Result<OperationOutcome, PicoError> {
        Err(PicoError::UnsupportedOperation {
            operation: "test".to_string(),
            kind: ProjectKind::VisualStudio.display_name().to_string(),
            reason: "vstest integration is not implemented in Raven Pico 0.1".to_string(),
        })
    }

    fn run(&self, _ctx: &OpContext) -> Result<OperationOutcome, PicoError> {
        Err(PicoError::UnsupportedOperation {
            operation: "run".to_string(),
            kind: ProjectKind::VisualStudio.display_name().to_string(),
            reason: "solutions do not define a single runnable output".to_string(),
        })
    }

    fn clean(&self, ctx: &OpContext) -> Result<OperationOutcome, PicoError> {
        let (prefix, tool) = self.resolve_tool(ctx)?;
        let tool_path = PathBuf::from(tool.path.clone().unwrap_or_default());
        support::execute(
            ctx,
            tool,
            self.msbuild_spec(ctx, &prefix, &tool_path, &["-t:Clean"]),
            Vec::new(),
            None,
        )
    }
}
