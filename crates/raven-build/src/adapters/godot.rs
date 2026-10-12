//! Adapter for Godot 4 projects (`project.godot`).
//!
//! * **run** launches the game: `godot --path <root>`.
//! * **test** requires a project-defined command (frameworks such as GUT or
//!   gdUnit are not assumed).
//! * **build** requires a project-defined command: exports need a preset
//!   name and an output path from `export_presets.cfg`, which Pico does not
//!   parse in 0.1.
//! * **clean** removes the generated `.godot` cache directory — the only
//!   directory Godot regenerates automatically.

use raven_core::error::PicoError;
use raven_core::kind::ProjectKind;

use crate::proc::ProcessSpec;
use crate::{Capabilities, OpContext, OperationOutcome, Profile, Support};

use super::support;

pub struct GodotAdapter;

const GODOT_HINT: &str = "install Godot 4: https://godotengine.org/download";

impl GodotAdapter {
    fn godot(
        &self,
        ctx: &OpContext,
    ) -> Result<(std::path::PathBuf, raven_core::report::ToolInfoDto), PicoError> {
        support::resolve(ctx, "godot", &["--version"], GODOT_HINT)
    }
}

impl crate::BuildAdapter for GodotAdapter {
    fn kind(&self) -> ProjectKind {
        ProjectKind::Godot
    }

    fn capabilities(&self, _ctx: &OpContext) -> Capabilities {
        Capabilities {
            build: Support::ConfigRequired {
                hint: Some(
                    "exports need a preset name and output path; set [build], e.g. \
                     command = \"godot\", args = [\"--headless\", \"--export-release\", \
                     \"Windows Desktop\", \"build/game.exe\"]"
                        .to_string(),
                ),
            },
            test: Support::ConfigRequired {
                hint: Some(
                    "install a test framework (GUT, gdUnit) and set [test] in raven-pico.toml"
                        .to_string(),
                ),
            },
            run: Support::Available,
            clean: Support::Available,
        }
    }

    fn build(&self, _ctx: &OpContext) -> Result<OperationOutcome, PicoError> {
        Err(PicoError::UnsupportedOperation {
            operation: "build".to_string(),
            kind: ProjectKind::Godot.display_name().to_string(),
            reason: "Godot exports require a preset name and output path".to_string(),
        })
    }

    fn test(&self, _ctx: &OpContext) -> Result<OperationOutcome, PicoError> {
        Err(PicoError::UnsupportedOperation {
            operation: "test".to_string(),
            kind: ProjectKind::Godot.display_name().to_string(),
            reason: "no universal Godot test runner; use GUT/gdUnit via [test]".to_string(),
        })
    }

    fn run(&self, ctx: &OpContext) -> Result<OperationOutcome, PicoError> {
        if ctx.profile == Profile::Release {
            return Err(PicoError::UnsupportedOperation {
                operation: "run".to_string(),
                kind: ProjectKind::Godot.display_name().to_string(),
                reason: "--release does not apply to running a Godot project".to_string(),
            });
        }
        let (path, tool) = self.godot(ctx)?;
        let spec = support::with_extra_args(ProcessSpec::new(path.display().to_string()), ctx)
            .arg("--path")
            .arg(ctx.root.display().to_string())
            .cwd(ctx.root.clone());
        support::execute(ctx, tool, spec, Vec::new(), None)
    }

    fn clean(&self, ctx: &OpContext) -> Result<OperationOutcome, PicoError> {
        let cache_dir = ctx.root.join(".godot");
        if !cache_dir.exists() {
            return Ok(OperationOutcome {
                tool: None,
                command: None,
                cwd: Some(ctx.root.clone()),
                result: None,
                artifacts: Vec::new(),
                note: Some("nothing to clean: .godot cache does not exist".to_string()),
            });
        }
        std::fs::remove_dir_all(&cache_dir)?;
        Ok(OperationOutcome {
            tool: None,
            command: None,
            cwd: Some(ctx.root.clone()),
            result: None,
            artifacts: Vec::new(),
            note: Some(format!("removed Godot cache {}", cache_dir.display())),
        })
    }
}
