//! Adapter for Lua projects.
//!
//! Lua has no standard build system; Pico's support is intentionally minimal:
//!
//! * **run** executes `main.lua` with an installed interpreter
//!   (`lua`, `lua5.4` ... `luajit`). If the project looks like a LÖVE game
//!   (`conf.lua` next to `main.lua`) but `love` is the intended runner, the
//!   error explains how to configure `[run]`.
//! * build / test / clean require project-defined commands.

use raven_core::error::PicoError;
use raven_core::kind::ProjectKind;
use raven_core::report::ToolInfoDto;

use crate::proc::ProcessSpec;
use crate::{Capabilities, OpContext, OperationOutcome, Profile, Support};

use super::support;

pub struct LuaAdapter;

const LUA_HINT: &str = "install Lua (https://www.lua.org/) or via your package manager";
const LOVE_HINT: &str = "run it with LÖVE by configuring [run]: command = \"love\", args = [\".\"]";

impl LuaAdapter {
    fn interpreter(&self, ctx: &OpContext) -> Result<(std::path::PathBuf, ToolInfoDto), PicoError> {
        for candidate in ["lua", "lua5.4", "lua5.3", "lua5.2", "luajit"] {
            if crate::resolve_tool(ctx, candidate, LUA_HINT).is_ok() {
                return support::resolve(ctx, candidate, &["-v"], LUA_HINT);
            }
        }
        Err(PicoError::ToolMissing {
            tool: "lua".to_string(),
            hint: LUA_HINT.to_string(),
        })
    }
}

fn unsupported(op: &str, reason: &str) -> PicoError {
    PicoError::UnsupportedOperation {
        operation: op.to_string(),
        kind: ProjectKind::Lua.display_name().to_string(),
        reason: reason.to_string(),
    }
}

impl crate::BuildAdapter for LuaAdapter {
    fn kind(&self) -> ProjectKind {
        ProjectKind::Lua
    }

    fn capabilities(&self, _ctx: &OpContext) -> Capabilities {
        Capabilities {
            build: Support::ConfigRequired {
                hint: Some(
                    "Lua has no standard build system; set [build] in raven-pico.toml".to_string(),
                ),
            },
            test: Support::ConfigRequired {
                hint: Some(
                    "install busted (https://lunarmodules.github.io/busted/) and set [test] in \
                     raven-pico.toml"
                        .to_string(),
                ),
            },
            run: Support::Available,
            clean: Support::ConfigRequired {
                hint: Some("set [clean] in raven-pico.toml".to_string()),
            },
        }
    }

    fn build(&self, _ctx: &OpContext) -> Result<OperationOutcome, PicoError> {
        Err(unsupported("build", "Lua has no standard build system"))
    }

    fn test(&self, _ctx: &OpContext) -> Result<OperationOutcome, PicoError> {
        Err(unsupported(
            "test",
            "no universal Lua test runner (use busted via [test])",
        ))
    }

    fn run(&self, ctx: &OpContext) -> Result<OperationOutcome, PicoError> {
        if ctx.profile == Profile::Release {
            return Err(unsupported(
                "run",
                "--release does not apply to Lua scripts",
            ));
        }
        if !ctx.root.join("main.lua").is_file() {
            return Err(unsupported("run", "no main.lua entry point found"));
        }
        match self.interpreter(ctx) {
            Ok((path, tool)) => {
                let mut spec = ProcessSpec::new(path.display().to_string())
                    .arg(ctx.root.join("main.lua").display().to_string());
                spec = support::with_extra_args(spec, ctx).cwd(ctx.root.clone());
                support::execute(ctx, tool, spec, Vec::new(), None)
            }
            Err(missing) => {
                // A LÖVE project (conf.lua + main.lua) usually wants `love .`.
                if ctx.root.join("conf.lua").is_file()
                    && crate::resolve_tool(ctx, "love", "https://love2d.org").is_ok()
                {
                    Err(PicoError::UnsupportedOperation {
                        operation: "run".to_string(),
                        kind: ProjectKind::Lua.display_name().to_string(),
                        reason: format!(
                            "this looks like a LÖVE project (conf.lua found); {LOVE_HINT}"
                        ),
                    })
                } else {
                    Err(missing)
                }
            }
        }
    }

    fn clean(&self, _ctx: &OpContext) -> Result<OperationOutcome, PicoError> {
        Err(unsupported(
            "clean",
            "Lua projects have no standard generated output",
        ))
    }
}
