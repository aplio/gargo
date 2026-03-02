use crate::command::diff_server::{DiffServerCommand, DiffServerEvent, DiffServerHandle};
use crate::config::Config;
use crate::plugin::types::{Plugin, PluginCommandSpec, PluginContext, PluginEvent, PluginOutput};

pub struct DiffUiPlugin {
    commands: Vec<PluginCommandSpec>,
    handle: Option<DiffServerHandle>,
    auto_open_browser: bool,
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
            ],
            handle,
            auto_open_browser: config.plugin.diff_ui.auto_open_browser,
        }
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

        let result = match command_id {
            "server.start_diff" => handle.command_tx.send(DiffServerCommand::Start {
                project_root: ctx.project_root().to_path_buf(),
            }),
            "server.stop_diff" => handle.command_tx.send(DiffServerCommand::Stop),
            _ => return Vec::new(),
        };
        if result.is_err() {
            vec![PluginOutput::Message(
                "Failed to send diff server command".to_string(),
            )]
        } else {
            Vec::new()
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
                    out.push(PluginOutput::Message(format!(
                        "Diff server: http://127.0.0.1:{}/diff",
                        port
                    )));
                    if self.auto_open_browser {
                        out.push(PluginOutput::OpenUrl(format!(
                            "http://127.0.0.1:{}/diff",
                            port
                        )));
                    }
                }
                DiffServerEvent::Stopped => {
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
