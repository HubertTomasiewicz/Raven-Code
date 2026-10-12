//! The subset of the LSP data model Raven Code consumes.
//!
//! Types are intentionally minimal and permissive (`Option`-heavy): language
//! servers vary widely in what they send, and a strict model would reject
//! servers that omit optional fields. Unknown fields are ignored by serde.

use serde::{Deserialize, Serialize};

/// LSP text position (zero-based line and UTF-16 code unit offset).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Position {
    pub line: u32,
    pub character: u32,
}

/// A start/end pair of [`Position`]s.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Range {
    pub start: Position,
    pub end: Position,
}

/// A `file://` URI plus range — the target of "go to definition".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Location {
    pub uri: String,
    pub range: Range,
}

/// Diagnostic severity per LSP (1=Error .. 4=Hint).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticSeverity(pub i64);

impl DiagnosticSeverity {
    pub const ERROR: DiagnosticSeverity = DiagnosticSeverity(1);
    pub const WARNING: DiagnosticSeverity = DiagnosticSeverity(2);
    pub const INFORMATION: DiagnosticSeverity = DiagnosticSeverity(3);
    pub const HINT: DiagnosticSeverity = DiagnosticSeverity(4);

    pub fn as_str(self) -> &'static str {
        match self.0 {
            1 => "error",
            2 => "warning",
            3 => "info",
            4 => "hint",
            _ => "diagnostic",
        }
    }

    pub fn is_error(self) -> bool {
        self.0 == 1
    }

    pub fn is_warning(self) -> bool {
        self.0 == 2
    }
}

/// One published diagnostic.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Diagnostic {
    pub range: Range,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub severity: Option<DiagnosticSeverity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<serde_json::Value>,
}

/// `textDocument/publishDiagnostics` notification parameters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PublishDiagnostics {
    pub uri: String,
    #[serde(default)]
    pub diagnostics: Vec<Diagnostic>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<i64>,
}

/// A completion proposal.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompletionItem {
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub insert_text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub documentation: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_edit: Option<serde_json::Value>,
}

impl CompletionItem {
    /// The text the editor should insert (label when the server omitted it).
    pub fn insert_text_or_label(&self) -> &str {
        self.insert_text.as_deref().unwrap_or(&self.label)
    }
}

/// Hover contents: markdown/plain text string or a marked-up structure.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum HoverContents {
    /// `MarkupContent { kind, value }`
    Markup { kind: String, value: String },
    /// A plain string (`MarkupContent` collapsed).
    Scalar(String),
    /// Anything else a server invents; rendered lossily as JSON.
    Raw(serde_json::Value),
}

impl HoverContents {
    /// Best-effort plain/markdown text for display.
    pub fn text(&self) -> String {
        match self {
            HoverContents::Markup { value, .. } => value.clone(),
            HoverContents::Scalar(text) => text.clone(),
            HoverContents::Raw(value) => value.to_string(),
        }
    }
}

/// A hover response (`null` result is modeled as `None` at the call site).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Hover {
    pub contents: HoverContents,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range: Option<Range>,
}

/// Flat symbol information (`textDocument/documentSymbol` may also return a
/// hierarchy; Raven Code uses the flat form for its Symbols panel).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DocumentSymbol {
    pub name: String,
    pub kind: SymbolKind,
    pub location: Location,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub container_name: Option<String>,
}

/// LSP SymbolKind (subset relevant for display ordering/icons).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SymbolKind(pub i64);

impl SymbolKind {
    pub const FILE: SymbolKind = SymbolKind(1);
    pub const NAMESPACE: SymbolKind = SymbolKind(3);
    pub const CLASS: SymbolKind = SymbolKind(5);
    pub const METHOD: SymbolKind = SymbolKind(6);
    pub const FUNCTION: SymbolKind = SymbolKind(12);
    pub const VARIABLE: SymbolKind = SymbolKind(13);
    pub const CONSTANT: SymbolKind = SymbolKind(14);
    pub const STRUCT: SymbolKind = SymbolKind(23);

    /// A short category label for the Symbols panel.
    pub fn label(self) -> &'static str {
        match self.0 {
            1 => "file",
            2 => "module",
            3 | 4 => "namespace",
            5 | 23 => "class",
            6 => "method",
            8 => "field",
            9 => "ctor",
            10 => "enum",
            11 => "interface",
            12 => "function",
            13 => "variable",
            14 => "constant",
            15 => "string",
            16 => "number",
            17 => "bool",
            20 => "struct",
            21 => "event",
            22 => "operator",
            24 => "type param",
            _ => "symbol",
        }
    }
}

/// Parameters for `textDocument/completion` (the fields Raven Code sends).
#[derive(Debug, Clone, Serialize)]
pub struct CompletionParams {
    pub text_document: TextDocumentId,
    pub position: Position,
}

/// Parameters for `textDocument/hover`, `definition`, `documentSymbol`.
#[derive(Debug, Clone, Serialize)]
pub struct TextDocumentPositionParams {
    pub text_document: TextDocumentId,
    pub position: Position,
}

#[derive(Debug, Clone, Serialize)]
pub struct TextDocumentId {
    pub uri: String,
}

/// `initialize` params — the capability block is a fixed minimal stub; the
/// client honestly declares only what it implements (full-text sync).
#[derive(Debug, Clone, Serialize)]
pub struct InitializeParams {
    #[serde(rename = "processId")]
    pub process_id: Option<u32>,
    #[serde(rename = "rootUri")]
    pub root_uri: Option<String>,
    #[serde(rename = "workspaceFolders")]
    pub workspace_folders: Option<Vec<WorkspaceFolder>>,
    pub capabilities: serde_json::Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkspaceFolder {
    pub uri: String,
    pub name: String,
}

/// Standard sync text document item used by didOpen/didChange/didSave.
#[derive(Debug, Clone, Serialize)]
pub struct TextDocumentItem {
    pub uri: String,
    #[serde(rename = "languageId")]
    pub language_id: String,
    pub version: i64,
    pub text: String,
}

/// `didChange` params with full-text sync (single content change covering
/// the whole document, as declared by our `initialize` capabilities).
#[derive(Debug, Clone, Serialize)]
pub struct DidChangeParams {
    #[serde(rename = "textDocument")]
    pub text_document: VersionedTextDocument,
    #[serde(rename = "contentChanges")]
    pub content_changes: Vec<ContentChange>,
}

#[derive(Debug, Clone, Serialize)]
pub struct VersionedTextDocument {
    pub uri: String,
    pub version: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ContentChange {
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DidSaveParams {
    pub text_document: TextDocumentId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DidCloseParams {
    pub text_document: TextDocumentId,
}

/// `textDocument/formatting` params.
#[derive(Debug, Clone, Serialize)]
pub struct FormattingParams {
    pub text_document: TextDocumentId,
    pub options: FormattingOptions,
}

#[derive(Debug, Clone, Serialize)]
pub struct FormattingOptions {
    #[serde(rename = "tabSize")]
    pub tab_size: u32,
    #[serde(rename = "insertSpaces")]
    pub insert_spaces: bool,
}

/// A text edit returned by formatting (and used for completion insertions).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextEdit {
    pub range: Range,
    #[serde(rename = "newText")]
    pub new_text: String,
}
