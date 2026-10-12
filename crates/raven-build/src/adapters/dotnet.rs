//! Adapter for .NET projects (`.csproj`, `.fsproj`, `.vbproj`) via the
//! `dotnet` CLI.

use raven_core::error::PicoError;
use raven_core::kind::ProjectKind;

use crate::proc::ProcessSpec;
use crate::{Capabilities, OpContext, OperationOutcome, Profile, Support};

use super::support;

pub struct DotNetAdapter;

const DOTNET_HINT: &str = "install the .NET SDK: https://dotnet.microsoft.com/download";

impl DotNetAdapter {
    fn dotnet(
        &self,
        ctx: &OpContext,
    ) -> Result<(std::path::PathBuf, raven_core::report::ToolInfoDto), PicoError> {
        support::resolve(ctx, "dotnet", &["--version"], DOTNET_HINT)
    }

    fn spec(&self, ctx: &OpContext, command: &str) -> ProcessSpec {
        let mut spec = ProcessSpec::new("dotnet")
            .arg(command)
            .arg("-c")
            .arg(match ctx.profile {
                Profile::Debug => "Debug",
                Profile::Release => "Release",
            });
        spec = support::with_extra_args(spec, ctx);
        spec.cwd(ctx.root.clone())
    }
}

impl crate::BuildAdapter for DotNetAdapter {
    fn kind(&self) -> ProjectKind {
        ProjectKind::DotNet
    }

    fn capabilities(&self, _ctx: &OpContext) -> Capabilities {
        Capabilities {
            build: Support::Available,
            test: Support::Available,
            run: Support::Available,
            clean: Support::Available,
        }
    }

    fn build(&self, ctx: &OpContext) -> Result<OperationOutcome, PicoError> {
        let (path, tool) = self.dotnet(ctx)?;
        let mut spec = self.spec(ctx, "build");
        spec.program = path.display().to_string();
        support::execute(ctx, tool, spec, Vec::new(), None)
    }

    fn test(&self, ctx: &OpContext) -> Result<OperationOutcome, PicoError> {
        let (path, tool) = self.dotnet(ctx)?;
        let mut spec = self.spec(ctx, "test");
        spec.program = path.display().to_string();
        support::execute(ctx, tool, spec, Vec::new(), None)
    }

    fn run(&self, ctx: &OpContext) -> Result<OperationOutcome, PicoError> {
        let (path, tool) = self.dotnet(ctx)?;
        let mut spec = self.spec(ctx, "run");
        spec.program = path.display().to_string();
        support::execute(ctx, tool, spec, Vec::new(), None)
    }

    fn clean(&self, ctx: &OpContext) -> Result<OperationOutcome, PicoError> {
        let (path, tool) = self.dotnet(ctx)?;
        let mut spec = self.spec(ctx, "clean");
        spec.program = path.display().to_string();
        support::execute(ctx, tool, spec, Vec::new(), None)
    }
}
