//! Adapter for Java projects driven by Maven (`pom.xml`).

use raven_core::error::PicoError;
use raven_core::kind::ProjectKind;

use crate::proc::ProcessSpec;
use crate::{Capabilities, OpContext, OperationOutcome, Profile, Support};

use super::support;

pub struct MavenAdapter;

const MAVEN_HINT: &str = "install Maven: https://maven.apache.org/install.html";

impl MavenAdapter {
    fn mvn(
        &self,
        ctx: &OpContext,
    ) -> Result<(std::path::PathBuf, raven_core::report::ToolInfoDto), PicoError> {
        support::resolve(ctx, "mvn", &["-v"], MAVEN_HINT)
    }
}

impl crate::BuildAdapter for MavenAdapter {
    fn kind(&self) -> ProjectKind {
        ProjectKind::Maven
    }

    fn capabilities(&self, _ctx: &OpContext) -> Capabilities {
        Capabilities {
            build: Support::Available,
            test: Support::Available,
            run: Support::ConfigRequired {
                hint: Some(
                    "set [run] to your exec-maven-plugin invocation, e.g. \
                     command = \"mvn\", args = [\"exec:java\"]"
                        .to_string(),
                ),
            },
            clean: Support::Available,
        }
    }

    fn build(&self, ctx: &OpContext) -> Result<OperationOutcome, PicoError> {
        if ctx.profile == Profile::Release {
            return Err(PicoError::UnsupportedOperation {
                operation: "build".to_string(),
                kind: ProjectKind::Maven.display_name().to_string(),
                reason: "Maven profiles are project-defined; --release is not applicable"
                    .to_string(),
            });
        }
        let (path, tool) = self.mvn(ctx)?;
        let spec = support::with_extra_args(
            ProcessSpec::new(path.display().to_string()).arg("package"),
            ctx,
        )
        .cwd(ctx.root.clone());
        // NOTE: `mvn package` runs unit tests as part of the standard
        // lifecycle; documented in docs/toolchains.md.
        support::execute(ctx, tool, spec, Vec::new(), None)
    }

    fn test(&self, ctx: &OpContext) -> Result<OperationOutcome, PicoError> {
        let (path, tool) = self.mvn(ctx)?;
        let spec = support::with_extra_args(
            ProcessSpec::new(path.display().to_string()).arg("test"),
            ctx,
        )
        .cwd(ctx.root.clone());
        support::execute(ctx, tool, spec, Vec::new(), None)
    }

    fn run(&self, _ctx: &OpContext) -> Result<OperationOutcome, PicoError> {
        Err(PicoError::UnsupportedOperation {
            operation: "run".to_string(),
            kind: ProjectKind::Maven.display_name().to_string(),
            reason: "the main class is project-defined (exec-maven-plugin)".to_string(),
        })
    }

    fn clean(&self, ctx: &OpContext) -> Result<OperationOutcome, PicoError> {
        let (path, tool) = self.mvn(ctx)?;
        let spec = ProcessSpec::new(path.display().to_string())
            .arg("clean")
            .cwd(ctx.root.clone());
        support::execute(ctx, tool, spec, Vec::new(), None)
    }
}
