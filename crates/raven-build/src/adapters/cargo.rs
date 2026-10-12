//! Adapter for Rust projects driven by Cargo.

use raven_core::error::PicoError;
use raven_core::kind::ProjectKind;
use raven_core::report::ToolInfoDto;

use crate::proc::ProcessSpec;
use crate::{Capabilities, OpContext, OperationOutcome, Profile, Support};

use super::support;

pub struct CargoAdapter;

const CARGO_HINT: &str = "install Rust via rustup: https://rustup.rs";

impl CargoAdapter {
    fn cargo(&self, ctx: &OpContext) -> Result<(std::path::PathBuf, ToolInfoDto), PicoError> {
        support::resolve(ctx, "cargo", &["--version"], CARGO_HINT)
    }

    fn build_spec(
        &self,
        ctx: &OpContext,
        cargo_path: &std::path::Path,
        command: &str,
    ) -> ProcessSpec {
        let mut spec = ProcessSpec::new(cargo_path.display().to_string()).arg(command);
        if ctx.profile == Profile::Release {
            spec = spec.arg("--release");
        }
        support::with_extra_args(spec, ctx).cwd(ctx.root.clone())
    }
}

impl crate::BuildAdapter for CargoAdapter {
    fn kind(&self) -> ProjectKind {
        ProjectKind::Rust
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
        let (path, tool) = self.cargo(ctx)?;
        let profile_dir = match ctx.profile {
            Profile::Debug => "target/debug",
            Profile::Release => "target/release",
        };
        let artifacts = vec![ctx.root.join(profile_dir)];
        support::execute(
            ctx,
            tool,
            self.build_spec(ctx, &path, "build"),
            artifacts,
            None,
        )
    }

    fn test(&self, ctx: &OpContext) -> Result<OperationOutcome, PicoError> {
        let (path, tool) = self.cargo(ctx)?;
        support::execute(
            ctx,
            tool,
            self.build_spec(ctx, &path, "test"),
            Vec::new(),
            None,
        )
    }

    fn run(&self, ctx: &OpContext) -> Result<OperationOutcome, PicoError> {
        let (path, tool) = self.cargo(ctx)?;
        support::execute(
            ctx,
            tool,
            self.build_spec(ctx, &path, "run"),
            Vec::new(),
            None,
        )
    }

    fn clean(&self, ctx: &OpContext) -> Result<OperationOutcome, PicoError> {
        let (path, tool) = self.cargo(ctx)?;
        // `cargo clean` ignores --release; no extra args.
        let spec = ProcessSpec::new(path.display().to_string())
            .arg("clean")
            .cwd(ctx.root.clone());
        support::execute(ctx, tool, spec, Vec::new(), None)
    }
}
