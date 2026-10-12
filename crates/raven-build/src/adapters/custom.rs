//! Adapter for projects driven purely by `raven-pico.toml`
//! ([`ProjectKind::Custom`]).
//!
//! All native capabilities are "config required" — `execute_operation`
//! dispatches to the configured command before this adapter is ever asked.
//! If no configuration section exists for an operation, the error explains
//! exactly what to add.

use raven_core::error::PicoError;
use raven_core::kind::ProjectKind;

use crate::{Capabilities, OpContext, OperationOutcome, Support};

pub struct CustomAdapter;

fn config_required_error(op: &str) -> PicoError {
    PicoError::UnsupportedOperation {
        operation: op.to_string(),
        kind: ProjectKind::Custom.display_name().to_string(),
        reason: format!(
            "custom projects are driven by raven-pico.toml; add a [{op}] section \
             with a command to enable this operation"
        ),
    }
}

impl crate::BuildAdapter for CustomAdapter {
    fn kind(&self) -> ProjectKind {
        ProjectKind::Custom
    }

    fn capabilities(&self, _ctx: &OpContext) -> Capabilities {
        Capabilities {
            build: Support::ConfigRequired { hint: None },
            test: Support::ConfigRequired { hint: None },
            run: Support::ConfigRequired { hint: None },
            clean: Support::ConfigRequired { hint: None },
        }
    }

    fn build(&self, _ctx: &OpContext) -> Result<OperationOutcome, PicoError> {
        Err(config_required_error("build"))
    }

    fn test(&self, _ctx: &OpContext) -> Result<OperationOutcome, PicoError> {
        Err(config_required_error("test"))
    }

    fn run(&self, _ctx: &OpContext) -> Result<OperationOutcome, PicoError> {
        Err(config_required_error("run"))
    }

    fn clean(&self, _ctx: &OpContext) -> Result<OperationOutcome, PicoError> {
        Err(config_required_error("clean"))
    }
}
