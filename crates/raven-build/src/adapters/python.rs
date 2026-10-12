//! Adapter for Python projects.
//!
//! Deliberately conservative, per the Raven Pico rules:
//!
//! * **build** is not supported — Python is interpreted and Pico refuses to
//!   invent a universal build behaviour for it.
//! * **test** runs `pytest` when it is installed; otherwise a `[test]`
//!   section in `raven-pico.toml` is required.
//! * **run** executes `main.py` with the interpreter when present;
//!   otherwise a `[run]` section is required.
//! * **clean** is not supported (removing `__pycache__` trees recursively is
//!   too easy to get wrong for generated-but-precious directories).

use raven_core::error::PicoError;
use raven_core::kind::ProjectKind;

use crate::proc::ProcessSpec;
use crate::{Capabilities, OpContext, OperationOutcome, Profile, Support};

use super::support;

pub struct PythonAdapter;

const PYTHON_HINT: &str = "install Python 3: https://www.python.org/downloads/";

impl PythonAdapter {
    fn python(&self, ctx: &OpContext) -> Result<String, PicoError> {
        // Prefer python3 on Unix; the py launcher on Windows is handled by
        // PATHEXT probing of "python" (python.exe) and "py".
        let candidates: &[&str] = if cfg!(windows) {
            &["python", "py"]
        } else {
            &["python3", "python"]
        };
        for candidate in candidates {
            if crate::resolve_tool(ctx, candidate, PYTHON_HINT).is_ok() {
                return Ok(candidate.to_string());
            }
        }
        Err(PicoError::ToolMissing {
            tool: "python".to_string(),
            hint: PYTHON_HINT.to_string(),
        })
    }
}

impl crate::BuildAdapter for PythonAdapter {
    fn kind(&self) -> ProjectKind {
        ProjectKind::Python
    }

    fn capabilities(&self, _ctx: &OpContext) -> Capabilities {
        Capabilities {
            build: Support::Unsupported {
                reason: "Python projects have no build step; use test or run".to_string(),
                hint: None,
            },
            test: Support::Available,
            run: Support::Available,
            clean: Support::Unsupported {
                reason: "there is no standard Python clean operation".to_string(),
                hint: Some("configure [clean] in raven-pico.toml".to_string()),
            },
        }
    }

    fn build(&self, _ctx: &OpContext) -> Result<OperationOutcome, PicoError> {
        Err(PicoError::UnsupportedOperation {
            operation: "build".to_string(),
            kind: ProjectKind::Python.display_name().to_string(),
            reason: "Python projects have no build step; use test or run".to_string(),
        })
    }

    fn test(&self, ctx: &OpContext) -> Result<OperationOutcome, PicoError> {
        // pytest is only used when actually installed; otherwise the user is
        // asked to configure [test].
        if crate::resolve_tool(
            ctx,
            "pytest",
            "install pytest or configure [test] in raven-pico.toml",
        )
        .is_ok()
        {
            let (path, tool) = support::resolve(
                ctx,
                "pytest",
                &["--version"],
                "install pytest: https://docs.pytest.org/en/stable/getting-started.html",
            )?;
            let spec = support::with_extra_args(ProcessSpec::new(path.display().to_string()), ctx)
                .cwd(ctx.root.clone());
            return support::execute(ctx, tool, spec, Vec::new(), None);
        }
        Err(PicoError::UnsupportedOperation {
            operation: "test".to_string(),
            kind: ProjectKind::Python.display_name().to_string(),
            reason: "no test command configured and pytest was not found".to_string(),
        })
    }

    fn run(&self, ctx: &OpContext) -> Result<OperationOutcome, PicoError> {
        if ctx.profile == Profile::Release {
            return Err(PicoError::UnsupportedOperation {
                operation: "run".to_string(),
                kind: ProjectKind::Python.display_name().to_string(),
                reason: "--release does not apply to Python projects".to_string(),
            });
        }
        if !ctx.root.join("main.py").is_file() {
            return Err(PicoError::UnsupportedOperation {
                operation: "run".to_string(),
                kind: ProjectKind::Python.display_name().to_string(),
                reason: "no main.py entry point found".to_string(),
            });
        }
        let interpreter = self.python(ctx)?;
        let (path, tool) = support::resolve(ctx, &interpreter, &["--version"], PYTHON_HINT)?;
        let mut spec = ProcessSpec::new(path.display().to_string())
            .arg(ctx.root.join("main.py").display().to_string());
        spec = support::with_extra_args(spec, ctx).cwd(ctx.root.clone());
        support::execute(ctx, tool, spec, Vec::new(), None)
    }

    fn clean(&self, _ctx: &OpContext) -> Result<OperationOutcome, PicoError> {
        Err(PicoError::UnsupportedOperation {
            operation: "clean".to_string(),
            kind: ProjectKind::Python.display_name().to_string(),
            reason: "there is no standard Python clean operation".to_string(),
        })
    }
}
