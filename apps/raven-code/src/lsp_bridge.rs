//! The LSP bridge: connects the `raven-lsp` client crate to the editor UI.
//!
//! Behavior (brief §7):
//!
//! * servers are configured in settings under `[lsp.servers.<id>]` with the
//!   languages they handle — nothing is hardcoded,
//! * a server is started **lazily**, when a file of a configured language is
//!   opened — never at application launch, and opening a file never
//!   installs anything (a missing executable is reported, not downloaded),
//! * documents sync **full-text** on open/change (matching the capability
//!   we declare in `initialize`),
//! * diagnostics arrive via `publishDiagnostics` and are shown in the
//!   bottom panel; completion, hover, definition, symbols and formatting
//!   round-trips are routed through the same event loop.
//!
//! Untested server features are documented in docs/raven-code/limitations.md.

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use raven_lsp::client::{
    normalize_completion, normalize_locations, normalize_symbols, path_to_uri, uri_to_path,
};
use raven_lsp::messages::{
    CompletionParams, DidChangeParams, DidCloseParams, DidSaveParams, FormattingOptions,
    FormattingParams, TextDocumentId, TextDocumentItem, TextDocumentPositionParams,
};
use raven_lsp::{Client, ClientConfig, LspEvent};

use raven_editor::language::LanguageDef;

/// One diagnostic as shown in the panel.
#[allow(dead_code)] // `uri` is part of the panel data model
#[derive(Debug, Clone)]
pub struct UiDiagnostic {
    pub uri: String,
    pub range: raven_lsp::messages::Range,
    pub message: String,
    pub severity: raven_lsp::messages::DiagnosticSeverity,
    pub source: Option<String>,
}

/// A completion popup entry.
#[derive(Debug, Clone)]
pub struct UiCompletion {
    pub label: String,
    pub insert: String,
    pub detail: Option<String>,
}

/// State of one connected server.
struct ServerState {
    client: Client,
    pending: HashMap<u64, PendingRequest>,
}

enum PendingRequest {
    Completion { uri: String },
    Hover { uri: String },
    Definition { uri: String },
    Symbols { uri: String },
    Formatting { uri: String },
}

/// The bridge itself: owns clients and results the UI renders.
pub struct LspBridge {
    config: BTreeMap<String, ConfiguredServer>,
    servers: HashMap<String, ServerState>,
    /// diagnostics keyed by file path
    pub diagnostics: HashMap<PathBuf, Vec<UiDiagnostic>>,
    /// last completion list received (for the popup)
    pub completions: Option<(String, Vec<UiCompletion>)>,
    /// last hover text (uri + rendered markdown)
    pub hover: Option<(PathBuf, String)>,
    /// last document symbols (per uri)
    pub symbols: HashMap<PathBuf, Vec<(String, String, u32)>>, // name, kind label, line
    /// transient status line, e.g. "rust-analyzer: starting…"
    pub status: Option<(String, String)>,
    pub server_errors: Vec<String>,
    /// definition location waiting to be jumped to (drained per frame)
    pending_jump: Option<raven_lsp::messages::Location>,
    /// formatting edits waiting to be applied (drained per frame)
    pending_format: Option<(String, Vec<raven_lsp::messages::TextEdit>)>,
}

struct ConfiguredServer {
    command: String,
    args: Vec<String>,
    languages: Vec<String>,
}

impl LspBridge {
    pub fn new(settings: &raven_editor::settings::Settings) -> LspBridge {
        let mut config = BTreeMap::new();
        for (id, entry) in &settings.lsp.servers {
            config.insert(
                id.clone(),
                ConfiguredServer {
                    command: entry.command.clone(),
                    args: entry.args.clone(),
                    languages: entry
                        .languages
                        .iter()
                        .map(|lang| lang.to_ascii_lowercase())
                        .collect(),
                },
            );
        }
        LspBridge {
            config,
            servers: HashMap::new(),
            diagnostics: HashMap::new(),
            completions: None,
            hover: None,
            symbols: HashMap::new(),
            status: None,
            server_errors: Vec::new(),
            pending_jump: None,
            pending_format: None,
        }
    }

    /// Is a server configured for this language? (Does not start one.)
    pub fn configured_for(&self, language: &LanguageDef) -> bool {
        self.config
            .values()
            .any(|server| server.languages.iter().any(|id| id == language.id))
    }

    pub fn server_running(&self, language: &LanguageDef) -> bool {
        self.config
            .iter()
            .filter(|(_, server)| server.languages.iter().any(|id| id == language.id))
            .any(|(id, _)| self.servers.contains_key(id))
    }

    /// Human summary for the status bar.
    pub fn status_for(&self, language: Option<&LanguageDef>) -> String {
        let Some(language) = language else {
            return String::new();
        };
        for (id, server) in &self.config {
            if server
                .languages
                .iter()
                .any(|candidate| candidate == language.id)
            {
                return match self.servers.get(id) {
                    Some(_) => format!("{id}: ready"),
                    None => format!("{id}: configured"),
                };
            }
        }
        String::new()
    }

    /// Called when a document is opened. Starts the configured server for
    /// the language (lazily) and sends didOpen.
    pub fn document_opened(&mut self, language: &'static LanguageDef, path: &Path, text: &str) {
        let Some(config_id) = self.config_id_for(language) else {
            return;
        };
        if !self.ensure_started(&config_id) {
            return;
        }
        let Some(state) = self.servers.get_mut(&config_id) else {
            return;
        };
        let uri = path_to_uri(path);
        let _ = state.client.did_open(&TextDocumentItem {
            uri,
            language_id: language.id.to_string(),
            version: 1,
            text: text.to_string(),
        });
    }

    pub fn document_changed(
        &mut self,
        language: &LanguageDef,
        path: &Path,
        text: &str,
        version: i64,
    ) {
        let Some(config_id) = self.config_id_for(language) else {
            return;
        };
        let Some(state) = self.servers.get_mut(&config_id) else {
            return;
        };
        let _ = state.client.did_change(&DidChangeParams {
            text_document: raven_lsp::messages::VersionedTextDocument {
                uri: path_to_uri(path),
                version,
            },
            content_changes: vec![raven_lsp::messages::ContentChange {
                text: text.to_string(),
            }],
        });
    }

    pub fn document_saved(&mut self, language: &LanguageDef, path: &Path, text: &str) {
        let Some(config_id) = self.config_id_for(language) else {
            return;
        };
        let Some(state) = self.servers.get_mut(&config_id) else {
            return;
        };
        let _ = state.client.did_save(&DidSaveParams {
            text_document: TextDocumentId {
                uri: path_to_uri(path),
            },
            text: Some(text.to_string()),
        });
    }

    pub fn document_closed(&mut self, language: &LanguageDef, path: &Path) {
        let Some(config_id) = self.config_id_for(language) else {
            return;
        };
        let Some(state) = self.servers.get_mut(&config_id) else {
            return;
        };
        let _ = state.client.did_close(&DidCloseParams {
            text_document: TextDocumentId {
                uri: path_to_uri(path),
            },
        });
        self.diagnostics.remove(path);
    }

    pub fn request_completion(
        &mut self,
        language: &LanguageDef,
        path: &Path,
        line: u32,
        character: u32,
    ) {
        let Some(config_id) = self.config_id_for(language) else {
            return;
        };
        let Some(state) = self.servers.get_mut(&config_id) else {
            return;
        };
        let uri = path_to_uri(path);
        if let Ok(guard) = state.client.completion(&CompletionParams {
            text_document: TextDocumentId { uri: uri.clone() },
            position: raven_lsp::messages::Position { line, character },
        }) {
            state
                .pending
                .insert(guard.id, PendingRequest::Completion { uri });
        }
    }

    pub fn request_hover(
        &mut self,
        language: &LanguageDef,
        path: &Path,
        line: u32,
        character: u32,
    ) {
        let Some(config_id) = self.config_id_for(language) else {
            return;
        };
        let Some(state) = self.servers.get_mut(&config_id) else {
            return;
        };
        if let Ok(guard) = state.client.hover(&TextDocumentPositionParams {
            text_document: TextDocumentId {
                uri: path_to_uri(path),
            },
            position: raven_lsp::messages::Position { line, character },
        }) {
            state.pending.insert(
                guard.id,
                PendingRequest::Hover {
                    uri: path_to_uri(path),
                },
            );
        }
    }

    pub fn request_definition(
        &mut self,
        language: &LanguageDef,
        path: &Path,
        line: u32,
        character: u32,
    ) {
        let Some(config_id) = self.config_id_for(language) else {
            return;
        };
        let Some(state) = self.servers.get_mut(&config_id) else {
            return;
        };
        if let Ok(guard) = state.client.definition(&TextDocumentPositionParams {
            text_document: TextDocumentId {
                uri: path_to_uri(path),
            },
            position: raven_lsp::messages::Position { line, character },
        }) {
            state.pending.insert(
                guard.id,
                PendingRequest::Definition {
                    uri: path_to_uri(path),
                },
            );
        }
    }

    pub fn request_symbols(&mut self, language: &LanguageDef, path: &Path) {
        let Some(config_id) = self.config_id_for(language) else {
            return;
        };
        let Some(state) = self.servers.get_mut(&config_id) else {
            return;
        };
        if let Ok(guard) = state.client.document_symbols(&TextDocumentId {
            uri: path_to_uri(path),
        }) {
            state.pending.insert(
                guard.id,
                PendingRequest::Symbols {
                    uri: path_to_uri(path),
                },
            );
        }
    }

    pub fn request_formatting(
        &mut self,
        language: &LanguageDef,
        path: &Path,
        tab_size: u32,
        insert_spaces: bool,
    ) {
        let Some(config_id) = self.config_id_for(language) else {
            return;
        };
        let Some(state) = self.servers.get_mut(&config_id) else {
            return;
        };
        if let Ok(guard) = state.client.formatting(&FormattingParams {
            text_document: TextDocumentId {
                uri: path_to_uri(path),
            },
            options: FormattingOptions {
                tab_size,
                insert_spaces,
            },
        }) {
            state.pending.insert(
                guard.id,
                PendingRequest::Formatting {
                    uri: path_to_uri(path),
                },
            );
        }
    }

    /// Non-blocking event pump — called once per frame.
    pub fn poll(&mut self) {
        let ids: Vec<String> = self.servers.keys().cloned().collect();
        for id in ids {
            let Some(state) = self.servers.get_mut(&id) else {
                continue;
            };
            let mut exited = false;
            loop {
                match state.client.try_recv() {
                    Some(LspEvent::Response {
                        id: request_id,
                        result,
                    }) => {
                        let pending = state.pending.remove(&request_id);
                        match (pending, result) {
                            (Some(PendingRequest::Completion { uri }), Ok(result)) => {
                                let items = normalize_completion(&result);
                                self.completions = Some((
                                    uri,
                                    items
                                        .into_iter()
                                        .map(|item| {
                                            let insert = item.insert_text_or_label().to_string();
                                            UiCompletion {
                                                label: item.label,
                                                insert,
                                                detail: item.detail,
                                            }
                                        })
                                        .collect(),
                                ));
                            }
                            (Some(PendingRequest::Hover { uri }), Ok(result)) => {
                                if result.is_null() {
                                    self.hover = None;
                                } else if let Ok(hover) =
                                    serde_json::from_value::<raven_lsp::messages::Hover>(result)
                                {
                                    if let Some(hover_path) = uri_to_path(&uri) {
                                        self.hover = Some((hover_path, hover.contents.text()));
                                    }
                                }
                            }
                            (Some(PendingRequest::Definition { uri, .. }), Ok(result)) => {
                                let locations = normalize_locations(&result);
                                if let Some(first) = locations.first() {
                                    if first.uri == uri {
                                        let _ = uri; // same file; app jumps via range
                                    }
                                    self.pending_jump = Some(first.clone());
                                }
                            }
                            (Some(PendingRequest::Symbols { uri }), Ok(result)) => {
                                let symbols = normalize_symbols(&result);
                                let Some(symbol_path) = uri_to_path(&uri) else {
                                    return;
                                };
                                self.symbols.insert(
                                    symbol_path,
                                    symbols
                                        .into_iter()
                                        .map(|symbol| {
                                            (
                                                symbol.name,
                                                symbol.kind.label().to_string(),
                                                symbol.location.range.start.line,
                                            )
                                        })
                                        .collect(),
                                );
                            }
                            (Some(PendingRequest::Formatting { uri }), Ok(result)) => {
                                if let Ok(edits) = serde_json::from_value::<
                                    Vec<raven_lsp::messages::TextEdit>,
                                >(result)
                                {
                                    if !edits.is_empty() {
                                        self.pending_format = Some((uri, edits));
                                    }
                                }
                            }
                            (Some(pending), Err(message)) => {
                                self.server_errors.push(format!(
                                    "{} request failed: {message}",
                                    pending_kind(&pending)
                                ));
                                self.server_errors.truncate(20);
                            }
                            (None, _) => {}
                        }
                    }
                    Some(LspEvent::Notification { method, params }) => {
                        if method == "textDocument/publishDiagnostics" {
                            if let Ok(publish) = serde_json::from_value::<
                                raven_lsp::messages::PublishDiagnostics,
                            >(params)
                            {
                                let path = uri_to_path(&publish.uri);
                                if let Some(path) = path {
                                    let diags: Vec<UiDiagnostic> = publish
                                        .diagnostics
                                        .iter()
                                        .map(|diagnostic| UiDiagnostic {
                                            uri: publish.uri.clone(),
                                            range: diagnostic.range,
                                            message: diagnostic.message.clone(),
                                            severity: diagnostic
                                                .severity
                                                .unwrap_or(raven_lsp::messages::DiagnosticSeverity::INFORMATION),
                                            source: diagnostic.source.clone(),
                                        })
                                        .collect();
                                    self.diagnostics.insert(path, diags);
                                }
                            }
                        } else if method == "raven/internal/exited" {
                            exited = true;
                        }
                    }
                    Some(LspEvent::Exited { reason }) => {
                        self.server_errors.push(format!("{} exited: {reason}", id));
                        self.server_errors.truncate(20);
                        exited = true;
                    }
                    None => break,
                }
            }
            if exited {
                self.servers.remove(&id);
            }
        }
    }

    /// A definition location the UI should jump to (drained per frame).
    pub fn take_pending_jump(&mut self) -> Option<raven_lsp::messages::Location> {
        self.pending_jump.take()
    }

    /// Formatting edits the UI should apply (drained per frame).
    pub fn take_pending_format(&mut self) -> Option<(String, Vec<raven_lsp::messages::TextEdit>)> {
        self.pending_format.take()
    }

    /// Shuts every server down (editor exit path).
    pub fn shutdown(&mut self) {
        for (_, mut state) in self.servers.drain() {
            state.client.shutdown_sync();
        }
    }

    fn config_id_for(&self, language: &LanguageDef) -> Option<String> {
        self.config
            .iter()
            .find(|(_, server)| server.languages.iter().any(|id| id == language.id))
            .map(|(id, _)| id.clone())
    }

    fn ensure_started(&mut self, config_id: &str) -> bool {
        if self.servers.contains_key(config_id) {
            return true;
        }
        let Some(config) = self.config.get(config_id) else {
            return false;
        };
        self.status = Some((config_id.to_string(), "starting".to_string()));
        let client_config = ClientConfig::new(config_id, &config.command)
            .with_args(config.args.iter().map(String::as_str));
        let started = Client::spawn(client_config).and_then(|client| {
            client.initialize()?;
            Ok(client)
        });
        match started {
            Ok(client) => {
                self.status = Some((config_id.to_string(), "ready".to_string()));
                self.servers.insert(
                    config_id.to_string(),
                    ServerState {
                        client,
                        pending: HashMap::new(),
                    },
                );
                true
            }
            Err(error) => {
                self.server_errors.push(format!(
                    "could not start language server '{config_id}' ({}): {error}; \
                     install it and make sure it is on PATH, or fix [lsp.servers.{config_id}] in settings",
                    config.command
                ));
                self.status = Some((config_id.to_string(), "failed".to_string()));
                false
            }
        }
    }
}

fn pending_kind(pending: &PendingRequest) -> &'static str {
    match pending {
        PendingRequest::Completion { .. } => "completion",
        PendingRequest::Hover { .. } => "hover",
        PendingRequest::Definition { .. } => "definition",
        PendingRequest::Symbols { .. } => "symbols",
        PendingRequest::Formatting { .. } => "formatting",
    }
}
