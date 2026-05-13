use crate::command::diff_server::{DiffServerCommand, DiffServerEvent, DiffServerHandle};
use crate::config::Config;
use crate::plugin::types::{Plugin, PluginCommandSpec, PluginContext, PluginEvent, PluginOutput};

pub struct DiffUiPlugin {
    commands: Vec<PluginCommandSpec>,
    handle: Option<DiffServerHandle>,
    auto_open_browser: bool,
    port: Option<u16>,
    pending_open: Option<&'static str>,
}

impl DiffUiPlugin {
    pub fn new(config: &Config) -> Self {
        let handle = DiffServerHandle::new().ok();
        Self {
            commands: vec![
                PluginCommandSpec {
                    id: "server.start_diff".to_string(),
                    label: "Start Diff Server".to_string(),
                    category: Some("Server".to_string()),
                },
                PluginCommandSpec {
                    id: "server.stop_diff".to_string(),
                    label: "Stop Diff Server".to_string(),
                    category: Some("Server".to_string()),
                },
                PluginCommandSpec {
                    id: "server.open_compare".to_string(),
                    label: "Open Compare Branches".to_string(),
                    category: Some("Server".to_string()),
                },
            ],
            handle,
            auto_open_browser: config.plugin.diff_ui.auto_open_browser,
            port: None,
            pending_open: None,
        }
    }

    fn url_for(&self, port: u16, path: &str) -> String {
        format!("http://127.0.0.1:{}{}", port, path)
    }

    fn emit_open(&self, port: u16, path: &str, label: &str) -> Vec<PluginOutput> {
        let url = self.url_for(port, path);
        let mut out = vec![PluginOutput::Message(format!("{}: {}", label, url))];
        if self.auto_open_browser {
            out.push(PluginOutput::OpenUrl(url));
        }
        out
    }
}

impl Plugin for DiffUiPlugin {
    fn id(&self) -> &str {
        "diff_ui"
    }

    fn commands(&self) -> &[PluginCommandSpec] {
        &self.commands
    }

    fn on_command(&mut self, command_id: &str, ctx: &PluginContext) -> Vec<PluginOutput> {
        let Some(handle) = &self.handle else {
            return vec![PluginOutput::Message(
                "Diff server plugin unavailable".to_string(),
            )];
        };

        match command_id {
            "server.start_diff" => {
                if let Some(port) = self.port {
                    return self.emit_open(port, "/diff", "Diff server");
                }
                self.pending_open = Some("/diff");
                if handle
                    .command_tx
                    .send(DiffServerCommand::Start {
                        project_root: ctx.project_root().to_path_buf(),
                    })
                    .is_err()
                {
                    self.pending_open = None;
                    return vec![PluginOutput::Message(
                        "Failed to send diff server command".to_string(),
                    )];
                }
                Vec::new()
            }
            "server.open_compare" => {
                if let Some(port) = self.port {
                    return self.emit_open(port, "/compare", "Compare branches");
                }
                self.pending_open = Some("/compare");
                if handle
                    .command_tx
                    .send(DiffServerCommand::Start {
                        project_root: ctx.project_root().to_path_buf(),
                    })
                    .is_err()
                {
                    self.pending_open = None;
                    return vec![PluginOutput::Message(
                        "Failed to send diff server command".to_string(),
                    )];
                }
                Vec::new()
            }
            "server.stop_diff" => {
                if handle.command_tx.send(DiffServerCommand::Stop).is_err() {
                    return vec![PluginOutput::Message(
                        "Failed to send diff server command".to_string(),
                    )];
                }
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    fn on_event(&mut self, _event: &PluginEvent, _ctx: &PluginContext) -> Vec<PluginOutput> {
        Vec::new()
    }

    fn poll(&mut self, _ctx: &PluginContext) -> Vec<PluginOutput> {
        let Some(handle) = &self.handle else {
            return Vec::new();
        };
        let mut out = Vec::new();
        while let Ok(event) = handle.event_rx.try_recv() {
            match event {
                DiffServerEvent::Started { port } => {
                    self.port = Some(port);
                    let path = self.pending_open.take().unwrap_or("/diff");
                    let label = if path == "/compare" {
                        "Compare branches"
                    } else {
                        "Diff server"
                    };
                    out.extend(self.emit_open(port, path, label));
                }
                DiffServerEvent::Stopped => {
                    self.port = None;
                    self.pending_open = None;
                    out.push(PluginOutput::Message("Diff server stopped".to_string()));
                }
                DiffServerEvent::Error(msg) => {
                    out.push(PluginOutput::Message(format!("Server error: {}", msg)));
                }
            }
        }
        out
    }
}
