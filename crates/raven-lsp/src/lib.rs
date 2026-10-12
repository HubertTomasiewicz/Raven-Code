//! # raven-lsp
//!
//! Language Server Protocol client foundation for Raven Code.
//!
//! Design mirrors the rest of the Raven ecosystem: **synchronous core,
//! threads, no async runtime**, matching the decision documented in
//! `docs/architecture.md` for the process layer. One [`Client`] owns one
//! language-server child process:
//!
//! * a **reader thread** frames JSON-RPC messages from the server's stdout
//!   (`Content-Length` framing) and forwards typed events to a channel,
//! * a **writer thread** serializes outbound messages to the server's stdin,
//! * requests carry monotonic ids; the UI correlates responses by id,
//! * cancellation uses the standard `$/cancelRequest` notification,
//! * shutdown is a protocol-conformant `shutdown` → `exit` sequence with a
//!   hard kill fallback.
//!
//! The crate models only the LSP surface Raven Code consumes (diagnostics,
//! completion, hover, definition, document symbols, formatting). Anything
//! else arrives as a raw notification the client answers with `null` (the
//! protocol requires a response to server→client *requests*; ignoring them
//! outright would stall some servers).
//!
//! Servers are always **optional external processes**: this crate never
//! downloads or installs anything, and starting one is an explicit act by
//! the application layer.

pub mod client;
pub mod messages;

pub use client::{Client, ClientConfig, LspEvent, PendingGuard, ServerStatus};
pub use messages::{
    CompletionItem, Diagnostic, DiagnosticSeverity, DocumentSymbol, HoverContents, Location,
    Position, PublishDiagnostics, Range, SymbolKind,
};

use thiserror::Error;

/// Errors surfaced by the client. `Protocol` covers malformed framing and
/// invalid payloads; the message always includes enough context to debug a
/// misbehaving server.
#[derive(Debug, Error)]
pub enum LspError {
    #[error("failed to start language server '{command}': {source}")]
    Spawn {
        command: String,
        #[source]
        source: std::io::Error,
    },
    #[error("language server protocol error: {0}")]
    Protocol(String),
    #[error("language server closed unexpectedly: {0}")]
    Disconnected(String),
    #[error("operation timed out waiting for the language server: {0}")]
    Timeout(String),
}
