//! Adapter for Java projects driven by Gradle.
//!
//! The Gradle wrapper (`gradlew` / `gradlew.bat`) is preferred over a global
//! Gradle installation, matching Gradle best practice. `build` maps to
//! `assemble` (compile + package without running tests) so that
//! `raven-pico build` and `raven-pico test` stay orthogonal.

use raven_core::error::PicoError;
use raven_core::kind::ProjectKind;
use raven_core::report::ToolInfoDto;

use crate::proc::ProcessSpec;
use crate::{Capabilities, OpContext, OperationOutcome, Profile, Support};

use super::support;

pub struct GradleAdapter;

const GRADLE_HINT: &str =
    "install Gradle (https://gradle.org/install/) or generate a wrapper with `gradle wrapper`";

impl GradleAdapter {
    /// Resolves the invocation: the wrapper when present, otherwise a global
    /// Gradle. Returns the base spec (program + optional wrapper arg) and the
    /// tool info.
    fn base_invocation(&self, ctx: &OpContext) -> Result<(ProcessSpec, ToolInfoDto), PicoError> {
        let wrapper = if cfg!(windows) {
            ctx.root.join("gradlew.bat")
        } else {
            ctx.root.join("gradlew")
        };
        if wrapper.is_file() {
            // A wrapper that lost its executable bit still runs via sh; the
            // wrapper script resolves its own location from $0.
            #[cfg(unix)]
            let executable = std::fs::metadata(&wrapper)
                .map(|meta| {
                    use std::os::unix::fs::PermissionsExt;
                    meta.permissions().mode() & 0o111 != 0
                })
                .unwrap_or(false);
            #[cfg(unix)]
            let (spec, tool) = if executable {
                (ProcessSpec::new(wrapper.display().to_string()), "gradlew")
            } else {
                (
                    ProcessSpec::new("sh").arg(wrapper.display().to_string()),
                    "gradlew (via sh)",
                )
            };
            #[cfg(windows)]
            let (spec, tool) = (ProcessSpec::new(wrapper.display().to_string()), "gradlew");
            let info = ToolInfoDto {
                name: tool.to_string(),
                version: None,
                path: Some(wrapper.display().to_string()),
            };
            Ok((spec, info))
        } else {
            let (path, tool) = support::resolve(ctx, "gradle", &["--version"], GRADLE_HINT)?;
            Ok((ProcessSpec::new(path.display().to_string()), tool))
        }
    }

    fn task_invocation(
        &self,
        ctx: &OpContext,
        task: &str,
    ) -> Result<(ProcessSpec, ToolInfoDto), PicoError> {
        let (spec, tool) = self.base_invocation(ctx)?;
        let spec = support::with_extra_args(spec.arg(task), ctx).cwd(ctx.root.clone());
        Ok((spec, tool))
    }
}

impl crate::BuildAdapter for GradleAdapter {
    fn kind(&self) -> ProjectKind {
        ProjectKind::Gradle
    }

    fn capabilities(&self, _ctx: &OpContext) -> Capabilities {
        // Tool resolution happens per invocation: a missing wrapper AND a
        // missing global Gradle surface as a structured ToolMissing error
        // (exit code 4) from build/test/clean with an actionable hint.
        Capabilities {
            build: Support::Available,
            test: Support::Available,
            run: Support::ConfigRequired {
                hint: Some(
                    "`gradle run` only exists with the application plugin; set [run] in \
                     raven-pico.toml to be explicit"
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
                kind: ProjectKind::Gradle.display_name().to_string(),
                reason: "Gradle configurations are project-defined; --release is not applicable"
                    .to_string(),
            });
        }
        let (spec, tool) = self.task_invocation(ctx, "assemble")?;
        support::execute(ctx, tool, spec, Vec::new(), None)
    }

    fn test(&self, ctx: &OpContext) -> Result<OperationOutcome, PicoError> {
        let (spec, tool) = self.task_invocation(ctx, "test")?;
        support::execute(ctx, tool, spec, Vec::new(), None)
    }

    fn run(&self, _ctx: &OpContext) -> Result<OperationOutcome, PicoError> {
        Err(PicoError::UnsupportedOperation {
            operation: "run".to_string(),
            kind: ProjectKind::Gradle.display_name().to_string(),
            reason: "the main class is project-defined (application plugin)".to_string(),
        })
    }

    fn clean(&self, ctx: &OpContext) -> Result<OperationOutcome, PicoError> {
        let (spec, tool) = self.task_invocation(ctx, "clean")?;
        support::execute(ctx, tool, spec, Vec::new(), None)
    }
}
