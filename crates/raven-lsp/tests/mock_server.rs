//! End-to-end tests for the LSP client against a mock server.
//!
//! The mock server is a real child process: the test binary re-executes
//! itself with `RAVEN_LSP_MOCK=1`, and the child runs
//! [`mock_main`] instead of the tests, speaking proper Content-Length
//! JSON-RPC on stdio. This exercises the actual framing, threading and
//! lifecycle code paths — no network, no real language server required.

use std::io::{BufRead, BufReader, Read, Write};

fn mock_mode() -> bool {
    std::env::var("RAVEN_LSP_MOCK").is_ok()
}

fn main() {
    if mock_mode() {
        mock_main();
        return;
    }
    full_client_lifecycle();
}

fn full_client_lifecycle() {
    let exe = std::env::current_exe().unwrap();

    // Run this binary directly and pass the marker portably on every OS.
    let config = raven_lsp::ClientConfig::new("mock", exe.to_string_lossy().as_ref())
        .with_env([("RAVEN_LSP_MOCK", "1")]);
    let mut client = raven_lsp::Client::spawn(config).expect("spawn client");

    let capabilities = client.initialize().expect("initialize handshake");
    assert_eq!(capabilities["capabilities"]["textDocumentSync"], 1);

    // didOpen triggers a publishDiagnostics notification from the mock.
    client
        .did_open(&raven_lsp::messages::TextDocumentItem {
            uri: "file:///mock/main.rs".into(),
            language_id: "rust".into(),
            version: 1,
            text: "let error_prone = true;\n".into(),
        })
        .unwrap();

    let mut saw_diagnostics = false;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while std::time::Instant::now() < deadline && !saw_diagnostics {
        match client.try_recv() {
            Some(raven_lsp::LspEvent::Notification { method, params }) => {
                if method == "textDocument/publishDiagnostics" {
                    let diags: raven_lsp::messages::PublishDiagnostics =
                        serde_json::from_value(params).unwrap();
                    assert_eq!(diags.uri, "file:///mock/main.rs");
                    assert_eq!(diags.diagnostics.len(), 1);
                    assert!(diags.diagnostics[0].message.contains("error_prone"));
                    saw_diagnostics = true;
                }
            }
            Some(_) => {}
            None => std::thread::sleep(std::time::Duration::from_millis(20)),
        }
    }
    assert!(saw_diagnostics, "publishDiagnostics never arrived");

    // Completion round-trip with id correlation.
    let guard = client
        .completion(&raven_lsp::messages::CompletionParams {
            text_document: raven_lsp::messages::TextDocumentId {
                uri: "file:///mock/main.rs".into(),
            },
            position: raven_lsp::messages::Position {
                line: 0,
                character: 4,
            },
        })
        .unwrap();

    let items = wait_for_response(&client, guard.id, |result| {
        raven_lsp::client::normalize_completion(&result)
    })
    .expect("completion response");
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].label, "error_prone");

    // Hover returns markdown.
    let guard = client
        .hover(&raven_lsp::messages::TextDocumentPositionParams {
            text_document: raven_lsp::messages::TextDocumentId {
                uri: "file:///mock/main.rs".into(),
            },
            position: raven_lsp::messages::Position {
                line: 0,
                character: 4,
            },
        })
        .unwrap();
    let hover = wait_for_response(&client, guard.id, |result| {
        serde_json::from_value::<raven_lsp::messages::Hover>(result).unwrap()
    })
    .expect("hover response");
    assert!(hover.contents.text().contains("error_prone"));

    // Definition returns one location in the same file.
    let guard = client
        .definition(&raven_lsp::messages::TextDocumentPositionParams {
            text_document: raven_lsp::messages::TextDocumentId {
                uri: "file:///mock/main.rs".into(),
            },
            position: raven_lsp::messages::Position {
                line: 0,
                character: 4,
            },
        })
        .unwrap();
    let locations = wait_for_response(&client, guard.id, |result| {
        raven_lsp::client::normalize_locations(&result)
    })
    .expect("definition response");
    assert_eq!(locations.len(), 1);
    assert_eq!(locations[0].range.start.line, 0);

    // Document symbols (hierarchical shape from the mock).
    let guard = client
        .document_symbols(&raven_lsp::messages::TextDocumentId {
            uri: "file:///mock/main.rs".into(),
        })
        .unwrap();
    let symbols = wait_for_response(&client, guard.id, |result| {
        raven_lsp::client::normalize_symbols(&result)
    })
    .expect("symbols response");
    assert!(symbols.iter().any(|symbol| symbol.name == "main"));
    assert!(symbols.iter().any(|symbol| symbol.name == "nested_helper"));

    // Graceful shutdown: shutdown -> exit; Drop kills if needed.
    client.shutdown_sync();
    println!("mock server end-to-end test passed");
}

fn wait_for_response<T>(
    client: &raven_lsp::Client,
    id: u64,
    convert: impl Fn(serde_json::Value) -> T,
) -> Option<T> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        if std::time::Instant::now() > deadline {
            return None;
        }
        match client.try_recv() {
            Some(raven_lsp::LspEvent::Response {
                id: response_id,
                result,
            }) if response_id == id => {
                return match result {
                    Ok(value) => Some(convert(value)),
                    Err(message) => panic!("server error for request {id}: {message}"),
                };
            }
            Some(_) => {}
            None => std::thread::sleep(std::time::Duration::from_millis(20)),
        }
    }
}

// ------------------------------------------------------------- mock server ----

fn read_frame_length(reader: &mut impl BufRead) -> Option<usize> {
    let mut length: Option<usize> = None;
    loop {
        let mut line = String::new();
        let bytes = reader.read_line(&mut line).ok()?;
        if bytes == 0 {
            return None;
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            return length;
        }
        if let Some((name, value)) = trimmed.split_once(':') {
            if name.trim().eq_ignore_ascii_case("content-length") {
                length = value.trim().parse().ok();
            }
        }
    }
}

fn write_frame(writer: &mut impl Write, body: &[u8]) -> std::io::Result<()> {
    write!(writer, "Content-Length: {}\r\n\r\n", body.len())?;
    writer.write_all(body)?;
    writer.flush()
}

fn send(writer: &mut impl Write, value: &serde_json::Value) {
    let body = serde_json::to_vec(value).unwrap();
    let _ = write_frame(writer, &body);
}

/// A tiny but protocol-correct language server used only by tests.
fn mock_main() {
    let stdin = std::io::stdin();
    let mut reader = BufReader::new(stdin.lock());
    let stdout = std::io::stdout();
    let mut writer = stdout.lock();

    loop {
        let Some(length) = read_frame_length(&mut reader) else {
            return;
        };
        let mut body = vec![0u8; length];
        if reader.read_exact(&mut body).is_err() {
            return;
        }
        let Ok(message) = serde_json::from_slice::<serde_json::Value>(&body) else {
            continue;
        };
        let method = message
            .get("method")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");

        match method {
            "initialize" => {
                let id = message["id"].as_u64().unwrap_or(0);
                send(
                    &mut writer,
                    &serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": { "capabilities": { "textDocumentSync": 1 } }
                    }),
                );
            }
            "initialized" => {}
            "textDocument/didOpen" => {
                let uri = message["params"]["textDocument"]["uri"]
                    .as_str()
                    .unwrap_or("")
                    .to_string();
                send(
                    &mut writer,
                    &serde_json::json!({
                        "jsonrpc": "2.0",
                        "method": "textDocument/publishDiagnostics",
                        "params": {
                            "uri": uri,
                            "diagnostics": [ {
                                "range": { "start": {"line": 0, "character": 4},
                                           "end": {"line": 0, "character": 15} },
                                "severity": 2,
                                "message": "avoid naming things error_prone",
                                "source": "mock" } ]
                        }
                    }),
                );
            }
            "textDocument/completion" => {
                let id = message["id"].as_u64().unwrap_or(0);
                send(
                    &mut writer,
                    &serde_json::json!({
                        "jsonrpc": "2.0", "id": id,
                        "result": [ { "label": "error_prone", "kind": 13 },
                                    { "label": "other", "insert_text": "other()" } ]
                    }),
                );
            }
            "textDocument/hover" => {
                let id = message["id"].as_u64().unwrap_or(0);
                send(
                    &mut writer,
                    &serde_json::json!({
                        "jsonrpc": "2.0", "id": id,
                        "result": { "contents": { "kind": "markdown",
                                                  "value": "**error_prone**: bool" } }
                    }),
                );
            }
            "textDocument/definition" => {
                let id = message["id"].as_u64().unwrap_or(0);
                send(
                    &mut writer,
                    &serde_json::json!({
                        "jsonrpc": "2.0", "id": id,
                        "result": { "uri": "file:///mock/main.rs",
                                    "range": { "start": {"line": 0, "character": 4},
                                               "end": {"line": 0, "character": 15} } }
                    }),
                );
            }
            "textDocument/documentSymbol" => {
                let id = message["id"].as_u64().unwrap_or(0);
                send(
                    &mut writer,
                    &serde_json::json!({
                        "jsonrpc": "2.0", "id": id,
                        "result": [ {
                            "name": "main", "kind": 12,
                            "range": { "start": {"line": 0, "character": 0},
                                       "end": {"line": 9, "character": 1} },
                            "children": [ {
                                "name": "nested_helper", "kind": 12,
                                "range": { "start": {"line": 2, "character": 4},
                                           "end": {"line": 5, "character": 5} } } ] } ]
                    }),
                );
            }
            "shutdown" => {
                let id = message["id"].as_u64().unwrap_or(0);
                send(
                    &mut writer,
                    &serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": null }),
                );
            }
            "exit" => return,
            _ => {
                if message.get("id").is_some() && message.get("method").is_some() {
                    // Unknown server->client request: answer null (what
                    // raven-lsp does too) so simple servers do not stall.
                    send(
                        &mut writer,
                        &serde_json::json!({ "jsonrpc": "2.0", "id": message["id"], "result": null }),
                    );
                }
            }
        }
    }
}
