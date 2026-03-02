use super::*;

impl Editor {
    fn normalize_file_path_key(path: &std::path::Path) -> Option<String> {
        if let Ok(canon) = path.canonicalize() {
            return Some(canon.to_string_lossy().to_string());
        }
        Some(path.to_string_lossy().to_string())
    }

    fn active_file_key(&self) -> Option<String> {
        let path = self.documents[self.active_index].file_path.as_ref()?;
        Self::normalize_file_path_key(path)
    }

    pub fn set_lsp_diagnostics_for_path(
        &mut self,
        path: &std::path::Path,
        diagnostics: Vec<LspDiagnostic>,
    ) {
        let Some(path_key) = Self::normalize_file_path_key(path) else {
            return;
        };
        let mut line_severity: std::collections::HashMap<usize, LspSeverity> =
            std::collections::HashMap::new();
        let mut line_message: std::collections::HashMap<usize, String> =
            std::collections::HashMap::new();

        for diagnostic in diagnostics {
            let line = diagnostic.range_start_line;
            let severity = diagnostic.severity;
            let label = match diagnostic.source.as_deref() {
                Some(src) if !src.is_empty() => format!("{}: {}", src, diagnostic.message),
                _ => diagnostic.message,
            };

            match line_severity.get(&line).copied() {
                Some(existing) if existing.rank() >= severity.rank() => {}
                _ => {
                    line_severity.insert(line, severity);
                    line_message.insert(line, label);
                }
            }
        }

        self.diagnostics_by_path.insert(
            path_key,
            FileDiagnostics {
                line_severity,
                line_message,
            },
        );
    }

    pub fn clear_lsp_diagnostics_for_path(&mut self, path: &std::path::Path) {
        if let Some(path_key) = Self::normalize_file_path_key(path) {
            self.diagnostics_by_path.remove(&path_key);
        }
    }

    pub fn active_diagnostic_severity_by_line(
        &self,
    ) -> Option<&std::collections::HashMap<usize, LspSeverity>> {
        let key = self.active_file_key()?;
        self.diagnostics_by_path.get(&key).map(|d| &d.line_severity)
    }

    pub fn active_line_diagnostic_message(&self) -> Option<&str> {
        let key = self.active_file_key()?;
        let diagnostics = self.diagnostics_by_path.get(&key)?;
        let line = self.active_buffer().cursor_line();
        diagnostics.line_message.get(&line).map(|s| s.as_str())
    }
}
