use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use serde_json::{Value, json};

use crate::command::registry::{CommandContext, CommandEffect, CommandEntry, CommandRegistry};
use crate::core::lsp_types::{LspDiagnostic, LspLocation, LspSeverity};
use crate::input::action::{Action, AppAction, IntegrationAction};

#[derive(Debug, Clone)]
pub enum LspClientCommand {
    Start {
        project_root: PathBuf,
        command: String,
        args: Vec<String>,
    },
    Stop,
    SyncFull {
        uri: String,
        language_id: String,
        version: i32,
        text: String,
    },
    DidSave {
        uri: String,
        text: String,
    },
    DidClose {
        uri: String,
    },
    RequestHover {
        uri: String,
        line: u32,
        character_utf16: u32,
    },
    RequestDefinition {
        uri: String,
        line: u32,
        character_utf16: u32,
    },
    RequestReferences {
        uri: String,
        line: u32,
        character_utf16: u32,
    },
}

#[derive(Debug, Clone)]
pub enum LspClientEvent {
    Started,
    Stopped,
    PublishDiagnostics {
        uri: String,
        diagnostics: Vec<LspDiagnostic>,
    },
    HoverResult {
        contents: String,
    },
    DefinitionResult {
        locations: Vec<LspLocation>,
    },
    ReferencesResult {
        locations: Vec<LspLocation>,
    },
    Error(String),
}

pub struct LspClientHandle {
    pub command_tx: mpsc::Sender<LspClientCommand>,
    pub event_rx: mpsc::Receiver<LspClientEvent>,
    _worker_thread: Option<thread::JoinHandle<()>>,
}

impl LspClientHandle {
    pub fn new() -> Result<Self, String> {
        let (command_tx, command_rx) = mpsc::channel();
        let (event_tx, event_rx) = mpsc::channel();

        let worker = LspClientWorker::new(command_rx, event_tx);
        let worker_thread = thread::Builder::new()
            .name("lsp-client".to_string())
            .spawn(move || worker.run())
            .map_err(|e| format!("Failed to spawn LSP worker thread: {}", e))?;

        Ok(Self {
            command_tx,
            event_rx,
            _worker_thread: Some(worker_thread),
        })
    }
}

enum PendingRequest {
    Initialize,
    Hover,
    Definition,
    References,
}

struct LspClientWorker {
    command_rx: mpsc::Receiver<LspClientCommand>,
    event_tx: mpsc::Sender<LspClientEvent>,
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    incoming_rx: Option<mpsc::Receiver<Value>>,
    reader_thread: Option<thread::JoinHandle<()>>,
    pending: HashMap<i64, PendingRequest>,
    open_docs: HashSet<String>,
    queued_until_initialized: Vec<LspClientCommand>,
    next_request_id: i64,
    initialized: bool,
}

impl LspClientWorker {
    fn new(
        command_rx: mpsc::Receiver<LspClientCommand>,
        event_tx: mpsc::Sender<LspClientEvent>,
    ) -> Self {
        Self {
            command_rx,
            event_tx,
            child: None,
            stdin: None,
            incoming_rx: None,
            reader_thread: None,
            pending: HashMap::new(),
            open_docs: HashSet::new(),
            queued_until_initialized: Vec::new(),
            next_request_id: 1,
            initialized: false,
        }
    }

    fn run(mut self) {
        loop {
            self.drain_incoming_messages();

            match self.command_rx.recv_timeout(Duration::from_millis(16)) {
                Ok(cmd) => self.handle_command(cmd),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    self.stop_process();
                    break;
                }
            }
        }
    }

    fn handle_command(&mut self, command: LspClientCommand) {
        match command {
            LspClientCommand::Start {
                project_root,
                command,
                args,
            } => {
                self.start_process(&project_root, &command, &args);
            }
            LspClientCommand::Stop => self.stop_process(),
            other => {
                if self.stdin.is_none() {
                    let _ = self.event_tx.send(LspClientEvent::Error(
                        "LSP process is not running".to_string(),
                    ));
                    return;
                }
                if !self.initialized {
                    self.queued_until_initialized.push(other);
                    return;
                }
                self.handle_initialized_command(other);
            }
        }
    }

    fn handle_initialized_command(&mut self, command: LspClientCommand) {
        match command {
            LspClientCommand::SyncFull {
                uri,
                language_id,
                version,
                text,
            } => {
                if self.open_docs.contains(&uri) {
                    self.send_notification(
                        "textDocument/didChange",
                        json!({
                            "textDocument": { "uri": uri, "version": version },
                            "contentChanges": [{ "text": text }],
                        }),
                    );
                } else {
                    self.send_notification(
                        "textDocument/didOpen",
                        json!({
                            "textDocument": {
                                "uri": uri,
                                "languageId": language_id,
                                "version": version,
                                "text": text,
                            }
                        }),
                    );
                    self.open_docs.insert(uri);
                }
            }
            LspClientCommand::DidSave { uri, text } => {
                if self.open_docs.contains(&uri) {
                    self.send_notification(
                        "textDocument/didSave",
                        json!({
                            "textDocument": { "uri": uri },
                            "text": text,
                        }),
                    );
                }
            }
            LspClientCommand::DidClose { uri } => {
                if self.open_docs.remove(&uri) {
                    self.send_notification(
                        "textDocument/didClose",
                        json!({
                            "textDocument": { "uri": uri }
                        }),
                    );
                }
            }
            LspClientCommand::RequestHover {
                uri,
                line,
                character_utf16,
            } => {
                let request_id = self.next_request_id();
                self.pending.insert(request_id, PendingRequest::Hover);
                self.send_request(
                    request_id,
                    "textDocument/hover",
                    json!({
                        "textDocument": { "uri": uri },
                        "position": { "line": line, "character": character_utf16 },
                    }),
                );
            }
            LspClientCommand::RequestDefinition {
                uri,
                line,
                character_utf16,
            } => {
                let request_id = self.next_request_id();
                self.pending.insert(request_id, PendingRequest::Definition);
                self.send_request(
                    request_id,
                    "textDocument/definition",
                    json!({
                        "textDocument": { "uri": uri },
                        "position": { "line": line, "character": character_utf16 },
                    }),
                );
            }
            LspClientCommand::RequestReferences {
                uri,
                line,
                character_utf16,
            } => {
                let request_id = self.next_request_id();
                self.pending.insert(request_id, PendingRequest::References);
                self.send_request(
                    request_id,
                    "textDocument/references",
                    json!({
                        "textDocument": { "uri": uri },
                        "position": { "line": line, "character": character_utf16 },
                        "context": { "includeDeclaration": true },
                    }),
                );
            }
            LspClientCommand::Start { .. } | LspClientCommand::Stop => {}
        }
    }

    fn start_process(&mut self, project_root: &Path, command: &str, args: &[String]) {
        if self.stdin.is_some() {
            let _ = self.event_tx.send(LspClientEvent::Error(
                "LSP process is already running".to_string(),
            ));
            return;
        }

        let mut cmd = Command::new(command);
        cmd.args(args);

        let mut child = match cmd
            .current_dir(project_root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(child) => child,
            Err(e) => {
                let _ = self.event_tx.send(LspClientEvent::Error(format!(
                    "Failed to start LSP command '{}': {}",
                    command, e
                )));
                return;
            }
        };

        let stdin = match child.stdin.take() {
            Some(stdin) => stdin,
            None => {
                let _ = self.event_tx.send(LspClientEvent::Error(
                    "Failed to capture LSP stdin".to_string(),
                ));
                let _ = child.kill();
                let _ = child.wait();
                return;
            }
        };
        let stdout = match child.stdout.take() {
            Some(stdout) => stdout,
            None => {
                let _ = self.event_tx.send(LspClientEvent::Error(
                    "Failed to capture LSP stdout".to_string(),
                ));
                let _ = child.kill();
                let _ = child.wait();
                return;
            }
        };
        if let Some(stderr) = child.stderr.take() {
            thread::spawn(move || {
                let mut reader = BufReader::new(stderr);
                let mut line = String::new();
                while reader.read_line(&mut line).ok().unwrap_or(0) > 0 {
                    line.clear();
                }
            });
        }

        let (incoming_tx, incoming_rx) = mpsc::channel();
        let reader_thread = thread::Builder::new()
            .name("lsp-reader".to_string())
            .spawn(move || {
                read_lsp_messages(stdout, incoming_tx);
            })
            .ok();

        self.child = Some(child);
        self.stdin = Some(stdin);
        self.incoming_rx = Some(incoming_rx);
        self.reader_thread = reader_thread;
        self.pending.clear();
        self.open_docs.clear();
        self.queued_until_initialized.clear();
        self.initialized = false;
        self.next_request_id = 1;

        let request_id = self.next_request_id();
        self.pending.insert(request_id, PendingRequest::Initialize);
        self.send_request(
            request_id,
            "initialize",
            json!({
                "processId": std::process::id(),
                "clientInfo": { "name": "gargo", "version": env!("CARGO_PKG_VERSION") },
                "rootUri": path_to_file_uri(project_root).unwrap_or_default(),
                "capabilities": {
                    "textDocument": {
                        "publishDiagnostics": {},
                    },
                    "workspace": {},
                },
            }),
        );
    }

    fn stop_process(&mut self) {
        let shutdown_id = self.next_request_id();
        if let Some(stdin) = &mut self.stdin {
            let _ = write_json_rpc(
                stdin,
                &json!({
                    "jsonrpc": "2.0",
                    "method": "shutdown",
                    "id": shutdown_id,
                    "params": {}
                }),
            );
            let _ = write_json_rpc(
                stdin,
                &json!({
                    "jsonrpc": "2.0",
                    "method": "exit",
                    "params": {}
                }),
            );
        }

        if let Some(child) = &mut self.child {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.child = None;
        self.stdin = None;
        self.incoming_rx = None;
        if let Some(join) = self.reader_thread.take() {
            let _ = join.join();
        }
        self.pending.clear();
        self.open_docs.clear();
        self.queued_until_initialized.clear();
        self.initialized = false;
        let _ = self.event_tx.send(LspClientEvent::Stopped);
    }

    fn drain_incoming_messages(&mut self) {
        let Some(rx) = self.incoming_rx.as_ref() else {
            return;
        };
        let mut incoming = Vec::new();
        while let Ok(value) = rx.try_recv() {
            incoming.push(value);
        }
        for value in incoming {
            self.handle_incoming(value);
        }
    }

    fn handle_incoming(&mut self, msg: Value) {
        if let Some(method) = msg.get("method").and_then(Value::as_str) {
            if method == "textDocument/publishDiagnostics" {
                let params = msg.get("params").cloned().unwrap_or(Value::Null);
                self.handle_publish_diagnostics(params);
            }
            return;
        }

        let response_id = msg.get("id").and_then(Value::as_i64);
        let Some(response_id) = response_id else {
            return;
        };
        let Some(pending) = self.pending.remove(&response_id) else {
            return;
        };

        if let Some(err) = msg.get("error") {
            let _ = self.event_tx.send(LspClientEvent::Error(format!(
                "LSP request failed: {}",
                err
            )));
            return;
        }

        match pending {
            PendingRequest::Initialize => {
                self.initialized = true;
                self.send_notification("initialized", json!({}));
                let _ = self.event_tx.send(LspClientEvent::Started);

                let queued = std::mem::take(&mut self.queued_until_initialized);
                for cmd in queued {
                    self.handle_initialized_command(cmd);
                }
            }
            PendingRequest::Hover => {
                let contents = msg
                    .get("result")
                    .and_then(|v| v.get("contents"))
                    .map(extract_hover_text)
                    .unwrap_or_else(|| "No hover information".to_string());
                let _ = self.event_tx.send(LspClientEvent::HoverResult { contents });
            }
            PendingRequest::Definition => {
                let locations = extract_locations(msg.get("result"));
                let _ = self
                    .event_tx
                    .send(LspClientEvent::DefinitionResult { locations });
            }
            PendingRequest::References => {
                let locations = extract_locations(msg.get("result"));
                let _ = self
                    .event_tx
                    .send(LspClientEvent::ReferencesResult { locations });
            }
        }
    }

    fn handle_publish_diagnostics(&mut self, params: Value) {
        let uri = params
            .get("uri")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let diagnostics = params
            .get("diagnostics")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .map(|item| LspDiagnostic {
                        range_start_line: item
                            .get("range")
                            .and_then(|v| v.get("start"))
                            .and_then(|v| v.get("line"))
                            .and_then(Value::as_u64)
                            .unwrap_or(0) as usize,
                        range_start_character_utf16: item
                            .get("range")
                            .and_then(|v| v.get("start"))
                            .and_then(|v| v.get("character"))
                            .and_then(Value::as_u64)
                            .unwrap_or(0)
                            as usize,
                        message: item
                            .get("message")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                        severity: LspSeverity::from_lsp_code(
                            item.get("severity").and_then(Value::as_u64),
                        ),
                        source: item
                            .get("source")
                            .and_then(Value::as_str)
                            .map(|s| s.to_string()),
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        let _ = self
            .event_tx
            .send(LspClientEvent::PublishDiagnostics { uri, diagnostics });
    }

    fn send_request(&mut self, id: i64, method: &str, params: Value) {
        if let Some(stdin) = &mut self.stdin {
            let payload = json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": method,
                "params": params,
            });
            if let Err(e) = write_json_rpc(stdin, &payload) {
                let _ = self.event_tx.send(LspClientEvent::Error(format!(
                    "Failed to send LSP request '{}': {}",
                    method, e
                )));
            }
        }
    }

    fn send_notification(&mut self, method: &str, params: Value) {
        if let Some(stdin) = &mut self.stdin {
            let payload = json!({
                "jsonrpc": "2.0",
                "method": method,
                "params": params,
            });
            if let Err(e) = write_json_rpc(stdin, &payload) {
                let _ = self.event_tx.send(LspClientEvent::Error(format!(
                    "Failed to send LSP notification '{}': {}",
                    method, e
                )));
            }
        }
    }

    fn next_request_id(&mut self) -> i64 {
        let id = self.next_request_id;
        self.next_request_id += 1;
        id
    }
}

fn write_json_rpc(stdin: &mut ChildStdin, payload: &Value) -> Result<(), String> {
    let body = payload.to_string();
    let header = format!("Content-Length: {}\r\n\r\n", body.len());
    stdin
        .write_all(header.as_bytes())
        .and_then(|_| stdin.write_all(body.as_bytes()))
        .and_then(|_| stdin.flush())
        .map_err(|e| e.to_string())
}

fn read_lsp_messages(stdout: impl Read, incoming_tx: mpsc::Sender<Value>) {
    let mut reader = BufReader::new(stdout);

    loop {
        let mut content_length = None::<usize>;
        loop {
            let mut line = String::new();
            let Ok(n) = reader.read_line(&mut line) else {
                return;
            };
            if n == 0 {
                return;
            }
            let line = line.trim_end_matches(['\r', '\n']);
            if line.is_empty() {
                break;
            }
            if let Some(value) = line.strip_prefix("Content-Length:") {
                content_length = value.trim().parse::<usize>().ok();
            }
        }

        let Some(content_length) = content_length else {
            continue;
        };
        let mut body = vec![0u8; content_length];
        if reader.read_exact(&mut body).is_err() {
            return;
        }
        if let Ok(value) = serde_json::from_slice::<Value>(&body)
            && incoming_tx.send(value).is_err()
        {
            return;
        }
    }
}

fn extract_hover_text(value: &Value) -> String {
    match value {
        Value::String(s) => s.to_string(),
        Value::Object(map) => {
            if let Some(v) = map.get("value").and_then(Value::as_str) {
                return v.to_string();
            }
            if let Some(v) = map.get("language").and_then(Value::as_str) {
                return format!("({})", v);
            }
            String::new()
        }
        Value::Array(items) => {
            let parts: Vec<String> = items
                .iter()
                .map(extract_hover_text)
                .filter(|s| !s.trim().is_empty())
                .collect();
            parts.join("\n")
        }
        _ => String::new(),
    }
}

fn extract_locations(value: Option<&Value>) -> Vec<LspLocation> {
    match value {
        Some(Value::Array(items)) => items.iter().filter_map(parse_location).collect(),
        Some(Value::Object(_)) => value.and_then(parse_location).into_iter().collect(),
        _ => Vec::new(),
    }
}

fn parse_location(value: &Value) -> Option<LspLocation> {
    let uri = value.get("uri").and_then(Value::as_str)?.to_string();
    let line = value
        .get("range")
        .and_then(|v| v.get("start"))
        .and_then(|v| v.get("line"))
        .and_then(Value::as_u64)? as usize;
    let character_utf16 = value
        .get("range")
        .and_then(|v| v.get("start"))
        .and_then(|v| v.get("character"))
        .and_then(Value::as_u64)? as usize;
    Some(LspLocation {
        uri,
        line,
        character_utf16,
    })
}

pub fn path_to_file_uri(path: &Path) -> Option<String> {
    let canonical = path.canonicalize().ok()?;
    url::Url::from_file_path(canonical)
        .ok()
        .map(|u| u.to_string())
}

pub fn file_uri_to_path(uri: &str) -> Option<PathBuf> {
    let parsed = url::Url::parse(uri).ok()?;
    parsed.to_file_path().ok()
}

pub fn register(registry: &mut CommandRegistry) {
    registry.register(CommandEntry {
        id: "lsp.hover".into(),
        label: "LSP: Hover".into(),
        category: Some("LSP".into()),
        action: Box::new(|_ctx: &CommandContext| {
            CommandEffect::Action(Action::App(AppAction::Integration(
                IntegrationAction::RunPluginCommand {
                    id: "lsp.hover".to_string(),
                },
            )))
        }),
    });

    registry.register(CommandEntry {
        id: "lsp.goto_definition".into(),
        label: "LSP: Go to Definition".into(),
        category: Some("LSP".into()),
        action: Box::new(|_ctx: &CommandContext| {
            CommandEffect::Action(Action::App(AppAction::Integration(
                IntegrationAction::RunPluginCommand {
                    id: "lsp.goto_definition".to_string(),
                },
            )))
        }),
    });

    registry.register(CommandEntry {
        id: "lsp.find_references".into(),
        label: "LSP: Find References".into(),
        category: Some("LSP".into()),
        action: Box::new(|_ctx: &CommandContext| {
            CommandEffect::Action(Action::App(AppAction::Integration(
                IntegrationAction::RunPluginCommand {
                    id: "lsp.find_references".to_string(),
                },
            )))
        }),
    });

    registry.register(CommandEntry {
        id: "lsp.restart".into(),
        label: "LSP: Restart".into(),
        category: Some("LSP".into()),
        action: Box::new(|_ctx: &CommandContext| {
            CommandEffect::Action(Action::App(AppAction::Integration(
                IntegrationAction::RunPluginCommand {
                    id: "lsp.restart".to_string(),
                },
            )))
        }),
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    use std::{fs, time::Duration};
    use tempfile::tempdir;

    #[test]
    fn extract_hover_text_from_markup_content() {
        let value = json!({"kind": "markdown", "value": "hello"});
        assert_eq!(extract_hover_text(&value), "hello");
    }

    #[test]
    fn extract_locations_from_array() {
        let value = json!([
            {
                "uri": "file:///tmp/a.md",
                "range": { "start": { "line": 3, "character": 7 } }
            }
        ]);
        let locations = extract_locations(Some(&value));
        assert_eq!(locations.len(), 1);
        assert_eq!(locations[0].line, 3);
        assert_eq!(locations[0].character_utf16, 7);
    }

    #[test]
    fn path_uri_round_trip() {
        let path = std::env::current_dir().expect("cwd");
        let uri = path_to_file_uri(&path).expect("uri");
        let round_trip = file_uri_to_path(&uri).expect("path");
        assert_eq!(round_trip, path.canonicalize().expect("canonical"));
    }

    #[cfg(unix)]
    #[test]
    fn start_process_does_not_append_server_arg_when_args_empty() {
        let tmp = tempdir().expect("tempdir");
        let script_path = tmp.path().join("dump_args.sh");
        let args_path = tmp.path().join("args.txt");
        fs::write(
            &script_path,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"{}\"\n",
                args_path.display()
            ),
        )
        .expect("write script");
        let mut perms = fs::metadata(&script_path).expect("metadata").permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&script_path, perms).expect("chmod");

        let (_command_tx, command_rx) = mpsc::channel();
        let (event_tx, _event_rx) = mpsc::channel();
        let mut worker = LspClientWorker::new(command_rx, event_tx);
        worker.start_process(
            tmp.path(),
            script_path.to_str().expect("script utf8 path"),
            &[],
        );
        std::thread::sleep(Duration::from_millis(50));
        worker.stop_process();

        let args = fs::read_to_string(&args_path).unwrap_or_default();
        assert!(
            args.trim().is_empty(),
            "expected no extra args, got: {:?}",
            args
        );
    }
}
