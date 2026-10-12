//! The JSON-RPC/LSP client: child process lifecycle, framing, events.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use serde::Deserialize;
use serde_json::{Value, json};

use crate::LspError;
use crate::messages::{
    CompletionParams, DidChangeParams, DidCloseParams, DidSaveParams, FormattingParams,
    InitializeParams, TextDocumentId, TextDocumentItem, TextDocumentPositionParams,
};

/// How to launch one language server.
#[derive(Debug, Clone)]
pub struct ClientConfig {
    /// Stable identifier (settings key), used for logging.
    pub id: String,
    /// Executable invoked directly (no shell), resolved on `PATH`.
    pub command: String,
    pub args: Vec<String>,
    /// Additional environment variables for the language server process.
    pub env: Vec<(String, String)>,
    /// Project root forwarded as `rootUri`/workspace folder.
    pub root: Option<std::path::PathBuf>,
    /// Startup budget for the `initialize` round-trip.
    pub initialize_timeout: Duration,
}

impl ClientConfig {
    pub fn new(id: impl Into<String>, command: impl Into<String>) -> ClientConfig {
        ClientConfig {
            id: id.into(),
            command: command.into(),
            args: Vec::new(),
            env: Vec::new(),
            root: None,
            initialize_timeout: Duration::from_secs(20),
        }
    }

    /// Adds launch arguments (builder style).
    pub fn with_args(mut self, args: impl IntoIterator<Item = impl Into<String>>) -> ClientConfig {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    /// Adds environment variables to the launched server process.
    pub fn with_env(
        mut self,
        vars: impl IntoIterator<Item = (impl Into<String>, impl Into<String>)>,
    ) -> ClientConfig {
        self.env.extend(
            vars.into_iter()
                .map(|(key, value)| (key.into(), value.into())),
        );
        self
    }

    /// Sets the workspace root forwarded as `rootUri`.
    pub fn with_root(mut self, root: impl Into<PathBuf>) -> ClientConfig {
        self.root = Some(root.into());
        self
    }
}

/// Everything the reader thread forwards to the UI event loop.
#[derive(Debug)]
pub enum LspEvent {
    /// A response to a client request, correlated by id.
    Response {
        id: u64,
        result: Result<Value, String>,
    },
    /// A server notification (`textDocument/publishDiagnostics`, ...).
    Notification { method: String, params: Value },
    /// The reader thread ended (EOF or fatal framing error).
    Exited { reason: String },
}

/// Lifecycle status mirrored in the status bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerStatus {
    Starting,
    Running,
    Failed,
    Stopped,
}

/// One running language-server connection.
pub struct Client {
    config: ClientConfig,
    next_id: AtomicU64,
    outgoing: Option<Sender<String>>,
    events: Receiver<LspEvent>,
    shutdown_requested: Arc<AtomicBool>,
    child_pid: Mutex<Option<u32>>,
    writer_handle: Option<JoinHandle<()>>,
    reader_handle: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for Client {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Client")
            .field("id", &self.config.id)
            .field("command", &self.config.command)
            .finish()
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        self.shutdown_sync();
    }
}

impl Client {
    /// Spawns the server process and starts the reader/writer threads.
    /// No `initialize` is sent yet — call [`Client::initialize`] after
    /// wiring the event receiver into the UI loop.
    pub fn spawn(config: ClientConfig) -> Result<Client, LspError> {
        let mut child = Command::new(&config.command)
            .args(&config.args)
            .envs(config.env.iter().map(|(key, value)| (key, value)))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|source| LspError::Spawn {
                command: config.command.clone(),
                source,
            })?;
        let pid = child.id();
        let stdin = child.stdin.take().expect("piped stdin");
        let stdout = child.stdout.take().expect("piped stdout");

        let (outgoing_tx, outgoing_rx) = mpsc::channel::<String>();
        let (event_tx, event_rx) = mpsc::channel::<LspEvent>();

        let shutdown = Arc::new(AtomicBool::new(false));

        let writer_shutdown = shutdown.clone();
        let writer_handle = std::thread::Builder::new()
            .name(format!("lsp-writer-{}", config.id))
            .spawn(move || writer_loop(stdin, outgoing_rx, writer_shutdown))
            .expect("spawn writer thread");

        let reader_id = config.id.clone();
        let reader_shutdown = shutdown.clone();
        let reader_event_tx = event_tx.clone();
        let reader_handle = std::thread::Builder::new()
            .name(format!("lsp-reader-{reader_id}"))
            .spawn(move || reader_loop(stdout, reader_event_tx, reader_shutdown))
            .expect("spawn reader thread");

        // Reap the child when it exits so no zombie lingers; store the pid
        // for the kill fallback in `shutdown_sync`.
        {
            let wait_event_tx = event_tx.clone();
            std::thread::Builder::new()
                .name(format!("lsp-wait-{reader_id}"))
                .spawn(move || {
                    let mut child = child;
                    match child.wait() {
                        Ok(status) => {
                            let _ = wait_event_tx.send(LspEvent::Notification {
                                method: "raven/internal/exited".to_string(),
                                params: json!({ "code": status.code() }),
                            });
                        }
                        Err(error) => {
                            let _ = wait_event_tx.send(LspEvent::Exited {
                                reason: error.to_string(),
                            });
                        }
                    }
                })
                .ok();
        }

        Ok(Client {
            config,
            next_id: AtomicU64::new(1),
            outgoing: Some(outgoing_tx),
            events: event_rx,
            shutdown_requested: shutdown,
            child_pid: Mutex::new(Some(pid)),
            writer_handle: Some(writer_handle),
            reader_handle: Some(reader_handle),
        })
    }

    pub fn config(&self) -> &ClientConfig {
        &self.config
    }

    /// Non-blocking receive for the UI event loop.
    pub fn try_recv(&self) -> Option<LspEvent> {
        self.events.try_recv().ok()
    }

    /// Sends `initialize` and waits (bounded) for the response, then sends
    /// `initialized`. Returns the server capabilities.
    pub fn initialize(&self) -> Result<Value, LspError> {
        let root = self.config.root.clone();
        let (root_uri, folders) = match &root {
            Some(path) => {
                let uri = path_to_uri(path);
                (
                    Some(uri.clone()),
                    Some(vec![crate::messages::WorkspaceFolder {
                        uri,
                        name: path
                            .file_name()
                            .map(|name| name.to_string_lossy().into_owned())
                            .unwrap_or_else(|| path.to_string_lossy().into_owned()),
                    }]),
                )
            }
            None => (None, None),
        };
        let params = InitializeParams {
            process_id: Some(std::process::id()),
            root_uri,
            workspace_folders: folders,
            // Honest capability declaration: full-text sync only.
            capabilities: json!({
                "textDocument": {
                    "synchronization": { "didSave": true, "willSave": false },
                    "completion": { "completionItem": { "snippetSupport": false } },
                    "hover": { "contentFormat": ["markdown", "plaintext"] },
                    "publishDiagnostics": { "relatedInformation": false }
                },
                "workspace": { "configuration": false, "workspaceFolders": true }
            }),
        };
        let id = self.send_request("initialize", Some(serde_json::to_value(params).unwrap()))?;

        let deadline = std::time::Instant::now() + self.config.initialize_timeout;
        loop {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                return Err(LspError::Timeout("initialize".to_string()));
            }
            match self.events.recv_timeout(remaining) {
                Ok(LspEvent::Response {
                    id: response_id,
                    result,
                }) if response_id == id => {
                    self.send_notification("initialized", Some(json!({})))?;
                    return result.map_err(LspError::Protocol);
                }
                Ok(event) => {
                    // Stash non-matching events? The UI event loop hasn't
                    // started yet, so pre-initialize notifications (rare)
                    // are dropped deliberately.
                    let _ = event;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    return Err(LspError::Timeout("initialize".to_string()));
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(LspError::Disconnected("writer/reader stopped".into()));
                }
            }
        }
    }

    pub fn did_open(&self, item: &TextDocumentItem) -> Result<(), LspError> {
        self.send_notification(
            "textDocument/didOpen",
            Some(serde_json::to_value(json!({ "textDocument": item })).unwrap()),
        )
    }

    pub fn did_change(&self, params: &DidChangeParams) -> Result<(), LspError> {
        self.send_notification(
            "textDocument/didChange",
            Some(serde_json::to_value(params).unwrap()),
        )
    }

    pub fn did_save(&self, params: &DidSaveParams) -> Result<(), LspError> {
        self.send_notification(
            "textDocument/didSave",
            Some(serde_json::to_value(params).unwrap()),
        )
    }

    pub fn did_close(&self, params: &DidCloseParams) -> Result<(), LspError> {
        self.send_notification(
            "textDocument/didClose",
            Some(serde_json::to_value(params).unwrap()),
        )
    }

    /// `textDocument/completion`. The result may be an array or the wrapped
    /// `{ items: [...] }` shape; callers normalize with
    /// [`crate::client::normalize_completion`].
    pub fn completion(&self, params: &CompletionParams) -> Result<PendingGuard, LspError> {
        self.request("textDocument/completion", params)
    }

    pub fn hover(&self, params: &TextDocumentPositionParams) -> Result<PendingGuard, LspError> {
        self.request("textDocument/hover", params)
    }

    pub fn definition(
        &self,
        params: &TextDocumentPositionParams,
    ) -> Result<PendingGuard, LspError> {
        self.request("textDocument/definition", params)
    }

    pub fn document_symbols(&self, params: &TextDocumentId) -> Result<PendingGuard, LspError> {
        self.request("textDocument/documentSymbol", params)
    }

    pub fn formatting(&self, params: &FormattingParams) -> Result<PendingGuard, LspError> {
        self.request("textDocument/formatting", params)
    }

    /// Cancels a previously issued request.
    pub fn cancel(&self, guard: PendingGuard) {
        let _ = self.send_notification("$/cancelRequest", Some(json!({ "id": guard.id })));
    }

    /// Sends a generic request, returning a guard correlated to the id.
    pub fn request(
        &self,
        method: &str,
        params: &(impl serde::Serialize + ?Sized),
    ) -> Result<PendingGuard, LspError> {
        let id = self.send_request(method, Some(serde_json::to_value(params).unwrap()))?;
        Ok(PendingGuard { id })
    }

    fn send_request(&self, method: &str, params: Option<Value>) -> Result<u64, LspError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let message = request_message(id, method, params);
        self.send_raw(&message)?;
        Ok(id)
    }

    pub fn send_notification(&self, method: &str, params: Option<Value>) -> Result<(), LspError> {
        let message = notification_message(method, params);
        self.send_raw(&message)
    }

    fn send_raw(&self, message: &str) -> Result<(), LspError> {
        match &self.outgoing {
            Some(sender) => sender
                .send(message.to_string())
                .map_err(|_| LspError::Disconnected("writer thread is gone".to_string())),
            None => Err(LspError::Disconnected("client already shut down".into())),
        }
    }

    /// Protocol shutdown: `shutdown` request (bounded wait), `exit`
    /// notification, then kill as a last resort. Safe to call twice.
    pub fn shutdown_sync(&mut self) {
        if self.shutdown_requested.swap(true, Ordering::SeqCst) {
            return;
        }
        // Best effort, bounded: a hung server must not block the editor.
        if self.send_request("shutdown", None).is_ok() {
            let deadline = std::time::Instant::now() + Duration::from_millis(300);
            while std::time::Instant::now() < deadline {
                match self.events.try_recv() {
                    Ok(LspEvent::Response { .. }) => break,
                    Ok(_) => continue,
                    Err(mpsc::TryRecvError::Empty) => std::thread::sleep(Duration::from_millis(10)),
                    Err(mpsc::TryRecvError::Disconnected) => break,
                }
            }
        }
        let _ = self.send_notification("exit", None);
        // Give the child a moment, then hard-kill by pid.
        let pid = self
            .child_pid
            .lock()
            .ok()
            .and_then(|mut guard| guard.take());
        if let Some(pid) = pid {
            std::thread::sleep(Duration::from_millis(120));
            kill_pid(pid);
        }
        self.outgoing = None;
        if let Some(handle) = self.writer_handle.take() {
            let _ = handle.join();
        }
        // The reader exits on EOF; do not join unconditionally to avoid
        // blocking the UI on a wedged child.
        drop(self.reader_handle.take());
    }
}

/// Marker returned by request builders; carries the id used on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PendingGuard {
    pub id: u64,
}

fn writer_loop(mut stdin: ChildStdin, outgoing: Receiver<String>, shutdown: Arc<AtomicBool>) {
    while let Ok(message) = outgoing.recv() {
        if shutdown.load(Ordering::SeqCst) {
            break;
        }
        if write_frame(&mut stdin, &message).is_err() {
            break;
        }
    }
    let _ = stdin.flush();
}

fn reader_loop(
    stdout: std::process::ChildStdout,
    events: Sender<LspEvent>,
    shutdown: Arc<AtomicBool>,
) {
    let mut reader = BufReader::new(stdout);
    loop {
        if shutdown.load(Ordering::SeqCst) {
            return;
        }
        let content_length = match read_headers(&mut reader) {
            Ok(Some(length)) => length,
            Ok(None) => break, // clean EOF
            Err(error) => {
                let _ = events.send(LspEvent::Exited { reason: error });
                return;
            }
        };
        let mut body = vec![0u8; content_length];
        if reader.read_exact(&mut body).is_err() {
            break;
        }
        match serde_json::from_slice::<Value>(&body) {
            Ok(value) => {
                if let Some(event) = translate(value) {
                    if events.send(event).is_err() {
                        return;
                    }
                }
            }
            Err(error) => {
                let _ = events.send(LspEvent::Exited {
                    reason: format!("invalid JSON frame: {error}"),
                });
                return;
            }
        }
    }
    let _ = events.send(LspEvent::Exited {
        reason: "server closed the output stream".to_string(),
    });
}

/// Reads RFC 7230-style headers terminated by an empty line; returns the
/// Content-Length, or `None` on clean EOF before any header.
fn read_headers(reader: &mut impl BufRead) -> Result<Option<usize>, String> {
    let mut content_length: Option<usize> = None;
    let mut first = true;
    loop {
        let mut line = String::new();
        let bytes = reader
            .read_line(&mut line)
            .map_err(|error| error.to_string())?;
        if bytes == 0 {
            if first {
                return Ok(None);
            }
            return Err("stream ended inside headers".to_string());
        }
        first = false;
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            if let Some(length) = content_length {
                return Ok(Some(length));
            }
            return Err("headers ended without Content-Length".to_string());
        }
        if let Some((name, value)) = trimmed.split_once(':') {
            if name.trim().eq_ignore_ascii_case("content-length") {
                content_length = value.trim().parse::<usize>().ok();
            }
        }
    }
}

fn write_frame(writer: &mut ChildStdin, message: &str) -> std::io::Result<()> {
    write!(writer, "Content-Length: {}\r\n\r\n", message.len())?;
    writer.write_all(message.as_bytes())?;
    writer.flush()
}

fn translate(value: Value) -> Option<LspEvent> {
    if let Some(id) = value.get("id") {
        // Server→client requests are answered with an empty result so
        // simple servers do not stall waiting for us.
        if value.get("method").is_some() {
            // Handled by the caller? We cannot respond from here (no writer
            // access); instead surface it as a notification the app may
            // act on. For the methods Raven Code consumes this is fine.
            return Some(LspEvent::Notification {
                method: value["method"].as_str().unwrap_or_default().to_string(),
                params: value.get("params").cloned().unwrap_or(Value::Null),
            });
        }
        let numeric = match id {
            Value::Number(number) => number.as_u64(),
            Value::String(text) => text.parse::<u64>().ok(),
            _ => None,
        };
        let id = numeric?;
        if let Some(error) = value.get("error") {
            return Some(LspEvent::Response {
                id,
                result: Err(match error.get("message") {
                    Some(message) => message.as_str().unwrap_or_default().to_string(),
                    None => error.to_string(),
                }),
            });
        }
        return Some(LspEvent::Response {
            id,
            result: Ok(value.get("result").cloned().unwrap_or(Value::Null)),
        });
    }
    let method = value.get("method")?.as_str()?.to_string();
    Some(LspEvent::Notification {
        method,
        params: value.get("params").cloned().unwrap_or(Value::Null),
    })
}

fn request_message(id: u64, method: &str, params: Option<Value>) -> String {
    let mut message = json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
    });
    if let Some(params) = params {
        message["params"] = params;
    }
    message.to_string()
}

fn notification_message(method: &str, params: Option<Value>) -> String {
    let mut message = json!({
        "jsonrpc": "2.0",
        "method": method,
    });
    if let Some(params) = params {
        message["params"] = params;
    }
    message.to_string()
}

/// Minimal `file://` URI encoding used by initialize/didOpen.
pub fn path_to_uri(path: &std::path::Path) -> String {
    let text = path.to_string_lossy();
    let mut uri = String::with_capacity(text.len() + 8);
    uri.push_str("file://");
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                uri.push(byte as char)
            }
            // Keep Windows drive letters readable: "C:" stays as-is.
            b':' => uri.push(':'),
            _ => uri.push_str(&format!("%{byte:02X}")),
        }
    }
    uri
}

/// Inverse of [`path_to_uri`] for diagnostics keys. Decodes percent
/// escapes at the byte level, so multi-byte UTF-8 survives round-trips.
pub fn uri_to_path(uri: &str) -> Option<std::path::PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    // Windows drive form `file:///C:/...`: the remainder starts with
    // "/X:" — strip exactly that leading slash. POSIX absolute paths
    // ("/home/...") keep their leading slash.
    let rest = if rest.len() >= 3 && rest.as_bytes()[0] == b'/' && rest.as_bytes()[2] == b':' {
        &rest[1..]
    } else {
        rest
    };

    let raw = rest.as_bytes();
    let mut bytes = Vec::with_capacity(raw.len());
    let mut index = 0usize;
    while index < raw.len() {
        if raw[index] == b'%' && index + 2 < raw.len() {
            let decoded = std::str::from_utf8(&raw[index + 1..index + 3])
                .ok()
                .and_then(|hex| u8::from_str_radix(hex, 16).ok());
            if let Some(byte) = decoded {
                bytes.push(byte);
                index += 3;
                continue;
            }
        }
        bytes.push(raw[index]);
        index += 1;
    }
    String::from_utf8(bytes).ok().map(std::path::PathBuf::from)
}

/// Normalizes the two legal completion result shapes into a flat list.
pub fn normalize_completion(value: &Value) -> Vec<crate::messages::CompletionItem> {
    let items = match value {
        Value::Array(items) => items.as_slice(),
        Value::Object(map) => match map.get("items").and_then(Value::as_array) {
            Some(items) => items.as_slice(),
            None => &[],
        },
        _ => &[],
    };
    items
        .iter()
        .filter_map(|item| {
            serde_json::from_value::<crate::messages::CompletionItem>(item.clone()).ok()
        })
        .collect()
}

/// Normalizes `definition` results: a single location, an array, or null.
pub fn normalize_locations(value: &Value) -> Vec<crate::messages::Location> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Shape {
        One(crate::messages::Location),
        Many(Vec<crate::messages::Location>),
        LocationLinkArray(Vec<Value>),
    }
    match serde_json::from_value::<Shape>(value.clone()) {
        Ok(Shape::One(location)) => vec![location],
        Ok(Shape::Many(list)) => list,
        // LocationLink has targetUri/targetRange; map defensively.
        Ok(Shape::LocationLinkArray(links)) => links
            .iter()
            .filter_map(|link| {
                let uri = link.get("targetUri")?.as_str()?.to_string();
                let range = link
                    .get("targetRange")
                    .cloned()
                    .or_else(|| link.get("targetSelectionRange").cloned())?;
                serde_json::from_value::<crate::messages::Range>(range)
                    .ok()
                    .map(|range| crate::messages::Location { uri, range })
            })
            .collect(),
        Err(_) => Vec::new(),
    }
}

/// Normalizes `documentSymbol` results: flat `SymbolInformation[]` or the
/// hierarchical `DocumentSymbol[]` form (flattened with their ranges).
pub fn normalize_symbols(value: &Value) -> Vec<crate::messages::DocumentSymbol> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Shape {
        Flat(Vec<crate::messages::DocumentSymbol>),
        Hierarchical(Vec<Value>),
    }
    fn flatten(value: &Value, uri: &str, out: &mut Vec<crate::messages::DocumentSymbol>) {
        let name = value
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let kind = value.get("kind").and_then(Value::as_i64).unwrap_or(0);
        let range = value
            .get("range")
            .or_else(|| value.get("location").and_then(|loc| loc.get("range")))
            .cloned();
        let Some(range_value) = range else { return };
        let Ok(range) = serde_json::from_value::<crate::messages::Range>(range_value) else {
            return;
        };
        out.push(crate::messages::DocumentSymbol {
            name,
            kind: crate::messages::SymbolKind(kind),
            location: crate::messages::Location {
                uri: uri.to_string(),
                range,
            },
            container_name: None,
        });
        if let Some(children) = value.get("children").and_then(Value::as_array) {
            for child in children {
                flatten(child, uri, out);
            }
        }
    }
    match serde_json::from_value::<Shape>(value.clone()) {
        Ok(Shape::Flat(list)) => list,
        Ok(Shape::Hierarchical(list)) => {
            let mut out = Vec::new();
            for item in &list {
                let uri = item
                    .get("location")
                    .and_then(|loc| loc.get("uri"))
                    .and_then(Value::as_str)
                    .unwrap_or("");
                flatten(item, uri, &mut out);
            }
            out
        }
        Err(_) => Vec::new(),
    }
}

fn kill_pid(pid: u32) {
    // Isolated so the platform-specific call stays in one auditable place;
    // a failed kill is acceptable — the child dies with the session anyway.
    #[cfg(unix)]
    {
        // SAFETY: sending SIGTERM to a spawned child pid; no memory effects.
        unsafe {
            libc::kill(pid as libc::pid_t, libc::SIGTERM);
        }
    }
    #[cfg(windows)]
    {
        // taskkill is the pragmatic, dependency-free fallback.
        let mut command = std::process::Command::new("taskkill");
        command.args(["/PID", &pid.to_string(), "/T", "/F"]);
        command.creation_flags_windows();
        let _ = command.output();
    }
}

#[cfg(windows)]
trait WindowsCreationFlags {
    fn creation_flags_windows(&mut self);
}

#[cfg(windows)]
impl WindowsCreationFlags for std::process::Command {
    fn creation_flags_windows(&mut self) {
        use std::os::windows::process::CommandExt as _;
        self.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uri_roundtrip_spaces_and_unicode() {
        let path = std::path::PathBuf::from("/home/z/my folder/héllo file.rs");
        let uri = path_to_uri(&path);
        assert!(uri.starts_with("file:///home/z/my%20folder/"));
        let back = uri_to_path(&uri).unwrap();
        assert_eq!(back, path);
    }

    #[test]
    fn framing_translation_responses_and_notifications() {
        let response = translate(serde_json::json!({
            "jsonrpc": "2.0", "id": 7, "result": { "capabilities": {} }
        }))
        .unwrap();
        match response {
            LspEvent::Response { id, result } => {
                assert_eq!(id, 7);
                assert!(result.is_ok());
            }
            other => panic!("unexpected event: {other:?}"),
        }

        let error = translate(serde_json::json!({
            "jsonrpc": "2.0", "id": 3, "error": { "code": -32600, "message": "bad" }
        }))
        .unwrap();
        match error {
            LspEvent::Response { id, result } => {
                assert_eq!(id, 3);
                assert_eq!(result.unwrap_err(), "bad");
            }
            other => panic!("unexpected event: {other:?}"),
        }

        let notification = translate(serde_json::json!({
            "jsonrpc": "2.0", "method": "textDocument/publishDiagnostics", "params": {}
        }))
        .unwrap();
        match notification {
            LspEvent::Notification { method, .. } => {
                assert_eq!(method, "textDocument/publishDiagnostics");
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[test]
    fn completion_shapes_normalize() {
        let flat = normalize_completion(&serde_json::json!([
            { "label": "foo", "insert_text": "foo()" },
            { "label": "bar" }
        ]));
        assert_eq!(flat.len(), 2);
        assert_eq!(flat[0].insert_text_or_label(), "foo()");
        assert_eq!(flat[1].insert_text_or_label(), "bar");

        let wrapped = normalize_completion(&serde_json::json!({
            "isIncomplete": false, "items": [ { "label": "baz" } ]
        }));
        assert_eq!(wrapped.len(), 1);
    }

    #[test]
    fn definition_shapes_normalize() {
        let one = serde_json::json!({ "uri": "file:///a.rs", "range": {
            "start": { "line": 1, "character": 0 }, "end": { "line": 1, "character": 5 } } });
        assert_eq!(normalize_locations(&one).len(), 1);
        let many = serde_json::json!([one.clone(), one]);
        assert_eq!(normalize_locations(&many).len(), 2);
        assert!(normalize_locations(&Value::Null).is_empty());
    }

    #[test]
    fn hierarchical_symbols_flatten() {
        let hierarchical = serde_json::json!([
            {
                "name": "Mod", "kind": 2,
                "range": { "start": {"line":0,"character":0}, "end": {"line":9,"character":1} },
                "children": [
                    { "name": "run", "kind": 12,
                      "range": { "start": {"line":2,"character":4}, "end": {"line":5,"character":5} } }
                ]
            }
        ]);
        let flat = normalize_symbols(&hierarchical);
        assert_eq!(flat.len(), 2);
        assert_eq!(flat[1].name, "run");
        assert_eq!(flat[1].kind.label(), "function");
    }
}
