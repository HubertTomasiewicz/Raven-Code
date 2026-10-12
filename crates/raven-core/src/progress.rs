//! Progress events and the sink trait future Raven applications plug into.
//!
//! Process execution, project detection and CLI presentation are deliberately
//! kept apart: library crates emit [`ProgressEvent`]s to an optional
//! [`ProgressSink`]; the CLI renders them to a terminal, while a future Raven
//! Hub / Studio service can render them over IPC or a socket without touching
//! the execution layer.

/// A progress event emitted while an operation runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProgressEvent {
    /// A human-readable stage change, e.g. "configuring build directory".
    Stage { message: String },
    /// A tool was resolved (found and, when cheap, version-probed).
    ToolResolved {
        tool: String,
        version: Option<String>,
    },
    /// A process is about to spawn.
    CommandSpawned { program: String, args: Vec<String> },
    /// A spawned process finished.
    CommandFinished {
        exit_code: Option<i32>,
        duration_ms: u64,
    },
    /// A non-fatal warning.
    Warning { message: String },
}

impl ProgressEvent {
    /// Short human-readable text for this event (used by the CLI sink).
    pub fn to_line(&self) -> String {
        match self {
            ProgressEvent::Stage { message } => message.clone(),
            ProgressEvent::ToolResolved { tool, version } => match version {
                Some(version) => format!("using {tool} {version}"),
                None => format!("using {tool}"),
            },
            ProgressEvent::CommandSpawned { program, args } => {
                if args.is_empty() {
                    program.clone()
                } else {
                    format!("{program} {}", args.join(" "))
                }
            }
            ProgressEvent::CommandFinished {
                exit_code,
                duration_ms,
            } => match exit_code {
                Some(0) => format!("done in {duration_ms} ms"),
                Some(code) => format!("exit code {code} after {duration_ms} ms"),
                None => format!("terminated after {duration_ms} ms"),
            },
            ProgressEvent::Warning { message } => format!("warning: {message}"),
        }
    }
}

/// Receiver for progress events.
///
/// Implementations must be usable from multiple threads; events are delivered
/// sequentially per operation.
pub trait ProgressSink: Send + Sync {
    fn on_event(&self, event: &ProgressEvent);
}

/// A sink that discards everything (used by library consumers and tests).
pub struct SilentSink;

impl ProgressSink for SilentSink {
    fn on_event(&self, _event: &ProgressEvent) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_lines_are_readable() {
        let spawned = ProgressEvent::CommandSpawned {
            program: "cargo".into(),
            args: vec!["build".into(), "--release".into()],
        };
        assert_eq!(spawned.to_line(), "cargo build --release");
        let finished = ProgressEvent::CommandFinished {
            exit_code: Some(2),
            duration_ms: 1500,
        };
        assert_eq!(finished.to_line(), "exit code 2 after 1500 ms");
    }
}
