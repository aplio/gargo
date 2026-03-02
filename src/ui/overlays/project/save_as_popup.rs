use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::input::action::{Action, AppAction, BufferAction, UiAction};
use crate::ui::framework::cell::CellStyle;
use crate::ui::framework::component::EventResult;
use crate::ui::framework::surface::Surface;
use crate::ui::shared::filtering::fuzzy_match;
use crate::core_lib::text::input::TextInput;
use crate::core_lib::ui::text::{display_width, truncate_to_width};

const MIN_POPUP_WIDTH: usize = 24;
const MIN_POPUP_HEIGHT: usize = 8;
const MAX_CANDIDATES: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveAsMode {
    SaveAs,
    Rename,
}

#[derive(Clone)]
struct SaveCandidate {
    path: String,
    name: String,
    is_dir: bool,
    score: i32,
}

pub struct SaveAsPopup {
    mode: SaveAsMode,
    project_root: PathBuf,
    input: TextInput,
    candidates: Vec<SaveCandidate>,
    selected: usize,
    selection_active: bool,
    error_message: Option<String>,
}

impl SaveAsPopup {
    pub fn new(default_path: String, project_root: PathBuf) -> Self {
        Self::with_mode(SaveAsMode::SaveAs, default_path, project_root)
    }

    pub fn new_rename(default_path: String, project_root: PathBuf) -> Self {
        Self::with_mode(SaveAsMode::Rename, default_path, project_root)
    }

    fn with_mode(mode: SaveAsMode, default_path: String, project_root: PathBuf) -> Self {
        let mut popup = Self {
            mode,
            project_root,
            input: TextInput::with_text(&default_path),
            candidates: Vec::new(),
            selected: 0,
            selection_active: false,
            error_message: None,
        };
        popup.refresh_candidates();
        popup
    }

    fn delete_prev_segment(&mut self) {
        if self.input.cursor == 0 {
            return;
        }
        let mut chars: Vec<char> = self.input.text.chars().collect();
        let mut start = self.input.cursor;
        while start > 0 && chars[start - 1] == '/' {
            start -= 1;
        }
        while start > 0 && chars[start - 1] != '/' {
            start -= 1;
        }
        if start < self.input.cursor {
            chars.drain(start..self.input.cursor);
            self.input.text = chars.into_iter().collect();
            self.input.cursor = start;
            self.selection_active = false;
            self.error_message = None;
            self.refresh_candidates();
        }
    }

    fn parse_parent_and_query(
        input: &str,
        project_root: &Path,
    ) -> Option<(PathBuf, String, bool, String)> {
        if input.is_empty() {
            return Some((
                project_root.to_path_buf(),
                String::new(),
                false,
                String::new(),
            ));
        }

        let input_path = Path::new(input);
        if input_path.is_absolute() {
            let path_str = input_path.to_string_lossy().to_string();
            if input.ends_with('/') {
                let parent = input_path.to_path_buf();
                let prefix = if path_str == "/" {
                    "/".to_string()
                } else {
                    format!("{}/", path_str.trim_end_matches('/'))
                };
                return Some((parent, String::new(), true, prefix));
            }
            let parent = input_path.parent()?.to_path_buf();
            let parent_display = parent.to_string_lossy().to_string();
            let query = input_path
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_default();
            let prefix = if parent_display == "/" {
                "/".to_string()
            } else {
                format!("{}/", parent_display)
            };
            return Some((parent, query, true, prefix));
        }

        if input.ends_with('/') {
            let rel_parent = PathBuf::from(input);
            let parent = project_root.join(&rel_parent);
            let prefix = rel_parent.to_string_lossy().to_string();
            return Some((parent, String::new(), false, prefix));
        }

        let rel_input = PathBuf::from(input);
        let rel_parent = rel_input
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_default();
        let parent = project_root.join(&rel_parent);
        let query = rel_input
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_default();
        let parent_prefix = if rel_parent.as_os_str().is_empty() {
            String::new()
        } else {
            format!("{}/", rel_parent.to_string_lossy())
        };
        Some((parent, query, false, parent_prefix))
    }

    fn refresh_candidates(&mut self) {
        self.candidates.clear();
        self.selected = 0;

        let Some((parent_dir, query, absolute_input, prefix)) =
            Self::parse_parent_and_query(&self.input.text, &self.project_root)
        else {
            return;
        };
        if !parent_dir.is_dir() {
            return;
        }

        let mut collected = Vec::new();
        if let Ok(entries) = std::fs::read_dir(parent_dir) {
            for entry in entries.flatten() {
                let Ok(ft) = entry.file_type() else {
                    continue;
                };
                let is_dir = ft.is_dir();
                let is_file = ft.is_file();
                if !(is_dir || is_file) {
                    continue;
                }
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with('.') {
                    continue;
                }

                let (score, matched) = if query.is_empty() {
                    (0, true)
                } else if let Some((score, _)) = fuzzy_match(&name, &query) {
                    (score, true)
                } else {
                    (0, false)
                };
                if !matched {
                    continue;
                }

                let path = if absolute_input {
                    if prefix == "/" {
                        format!("/{}", name)
                    } else {
                        format!("{}{}", prefix, name)
                    }
                } else {
                    format!("{}{}", prefix, name)
                };

                collected.push(SaveCandidate {
                    path,
                    name,
                    is_dir,
                    score,
                });
            }
        }

        collected.sort_by(|a, b| {
            b.score
                .cmp(&a.score)
                .then_with(|| b.is_dir.cmp(&a.is_dir))
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        collected.truncate(MAX_CANDIDATES);
        self.candidates = collected;
    }

    fn select_next(&mut self) {
        if self.candidates.is_empty() {
            return;
        }
        self.selection_active = true;
        self.selected = (self.selected + 1) % self.candidates.len();
    }

    fn select_prev(&mut self) {
        if self.candidates.is_empty() {
            return;
        }
        self.selection_active = true;
        self.selected = if self.selected == 0 {
            self.candidates.len() - 1
        } else {
            self.selected - 1
        };
    }

    fn complete_selected(&mut self) {
        if self.candidates.is_empty() {
            return;
        }
        let idx = if self.selection_active {
            self.selected
        } else {
            0
        };
        let mut completed = self.candidates[idx].path.clone();
        if self.candidates[idx].is_dir && !completed.ends_with('/') {
            completed.push('/');
        }
        self.input.set_text(completed);
        self.selection_active = false;
        self.error_message = None;
        self.refresh_candidates();
    }

    fn selected_submission_path(&self) -> String {
        if self.selection_active
            && !self.candidates.is_empty()
            && let Some(candidate) = self.candidates.get(self.selected)
        {
            return candidate.path.clone();
        }
        self.input.text.clone()
    }

    fn resolve_target_path(&self, path: &str) -> PathBuf {
        let p = PathBuf::from(path);
        if p.is_absolute() {
            p
        } else {
            self.project_root.join(p)
        }
    }

    fn validate_file_path(&self, path: &str) -> Result<String, String> {
        let trimmed = path.trim();
        if trimmed.is_empty() {
            return Err("Path is empty".to_string());
        }
        if trimmed.ends_with('/') {
            return Err("Path must be a file path".to_string());
        }

        let target = self.resolve_target_path(trimmed);
        if target.exists() && target.is_dir() {
            return Err("Path is a directory".to_string());
        }

        Ok(trimmed.to_string())
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> EventResult {
        if key.kind != KeyEventKind::Press {
            return EventResult::Consumed;
        }

        if key.modifiers.contains(KeyModifiers::CONTROL) {
            return match key.code {
                KeyCode::Char('q') | KeyCode::Char('c') => {
                    EventResult::Action(Action::Ui(UiAction::CloseSaveAsPopup))
                }
                KeyCode::Char('n') | KeyCode::Down => {
                    self.select_next();
                    EventResult::Consumed
                }
                KeyCode::Char('p') | KeyCode::Up => {
                    self.select_prev();
                    EventResult::Consumed
                }
                KeyCode::Char('f') => {
                    self.input.move_right();
                    EventResult::Consumed
                }
                KeyCode::Char('b') => {
                    self.input.move_left();
                    EventResult::Consumed
                }
                KeyCode::Char('a') => {
                    self.input.move_start();
                    EventResult::Consumed
                }
                KeyCode::Char('e') => {
                    self.input.move_end();
                    EventResult::Consumed
                }
                KeyCode::Char('w') => {
                    self.delete_prev_segment();
                    EventResult::Consumed
                }
                KeyCode::Char('k') => {
                    if self.input.delete_to_end() {
                        self.selection_active = false;
                        self.error_message = None;
                        self.refresh_candidates();
                    }
                    EventResult::Consumed
                }
                _ => EventResult::Consumed,
            };
        }

        match key.code {
            KeyCode::Esc => EventResult::Action(Action::Ui(UiAction::CloseSaveAsPopup)),
            KeyCode::Tab => {
                self.complete_selected();
                EventResult::Consumed
            }
            KeyCode::Up => {
                self.select_prev();
                EventResult::Consumed
            }
            KeyCode::Down => {
                self.select_next();
                EventResult::Consumed
            }
            KeyCode::Left => {
                self.input.move_left();
                EventResult::Consumed
            }
            KeyCode::Right => {
                self.input.move_right();
                EventResult::Consumed
            }
            KeyCode::Backspace => {
                if self.input.backspace() {
                    self.selection_active = false;
                    self.error_message = None;
                    self.refresh_candidates();
                }
                EventResult::Consumed
            }
            KeyCode::Enter => {
                let target = self.selected_submission_path();
                match self.validate_file_path(&target) {
                    Ok(path) => {
                        let buffer_action = match self.mode {
                            SaveAsMode::SaveAs => BufferAction::SaveBufferAs(path),
                            SaveAsMode::Rename => BufferAction::RenameBufferFile(path),
                        };
                        EventResult::Action(Action::App(AppAction::Buffer(buffer_action)))
                    }
                    Err(msg) => {
                        self.error_message = Some(msg);
                        EventResult::Consumed
                    }
                }
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::ALT) => {
                self.input.insert_char(c);
                self.selection_active = false;
                self.error_message = None;
                self.refresh_candidates();
                EventResult::Consumed
            }
            _ => EventResult::Consumed,
        }
    }

    pub fn render_overlay(&self, surface: &mut Surface) -> Option<(u16, u16)> {
        let cols = surface.width;
        let rows = surface.height;
        let popup_w = (cols * 85 / 100)
            .max(MIN_POPUP_WIDTH)
            .min(cols.saturating_sub(2));
        let popup_h = (rows * 60 / 100)
            .max(MIN_POPUP_HEIGHT)
            .min(rows.saturating_sub(2));
        let x = (cols.saturating_sub(popup_w)) / 2;
        let y = (rows.saturating_sub(popup_h)) / 2;
        let inner_w = popup_w.saturating_sub(2);

        let base = CellStyle::default();
        let dim = CellStyle {
            dim: true,
            ..CellStyle::default()
        };
        let selected = CellStyle {
            reverse: true,
            ..CellStyle::default()
        };
        let error = CellStyle {
            fg: Some(crossterm::style::Color::Red),
            ..CellStyle::default()
        };

        for row in 0..popup_h {
            if row == 0 {
                surface.put_str(x, y + row, "\u{250c}", &base);
                surface.fill_region(x + 1, y + row, inner_w, '\u{2500}', &base);
                surface.put_str(x + 1 + inner_w, y + row, "\u{2510}", &base);
            } else if row == popup_h - 1 {
                surface.put_str(x, y + row, "\u{2514}", &base);
                surface.fill_region(x + 1, y + row, inner_w, '\u{2500}', &base);
                surface.put_str(x + 1 + inner_w, y + row, "\u{2518}", &base);
            } else {
                surface.put_str(x, y + row, "\u{2502}", &base);
                surface.fill_region(x + 1, y + row, inner_w, ' ', &base);
                surface.put_str(x + 1 + inner_w, y + row, "\u{2502}", &base);
            }
        }

        let title = match self.mode {
            SaveAsMode::SaveAs => " Save Current Buffer As ",
            SaveAsMode::Rename => " Rename File In Buffer ",
        };
        surface.put_str(x + 2, y, title, &base);

        let input_row = y + 1;
        let prompt = "path: ";
        surface.put_str(x + 1, input_row, prompt, &base);
        let input_x = x + 1 + display_width(prompt);
        let input_room = inner_w.saturating_sub(display_width(prompt));
        let (truncated_input, _) = truncate_to_width(&self.input.text, input_room);
        surface.put_str(input_x, input_row, truncated_input, &base);

        let hint_row = y + 2;
        let hint = "ctrl-f/b move  ctrl-w/k delete  ctrl-n/p or up/down select  tab complete";
        let (hint_text, _) = truncate_to_width(hint, inner_w);
        surface.put_str(x + 1, hint_row, hint_text, &dim);

        let list_top = y + 3;
        let list_bottom = y + popup_h - 2;
        let list_h = list_bottom.saturating_sub(list_top);
        let mut start = 0usize;
        if self.selection_active && self.selected >= list_h && list_h > 0 {
            start = self.selected + 1 - list_h;
        }
        for row in 0..list_h {
            let idx = start + row;
            if idx >= self.candidates.len() {
                break;
            }
            let style = if self.selection_active && idx == self.selected {
                &selected
            } else {
                &base
            };
            let marker = if self.selection_active && idx == self.selected {
                "> "
            } else {
                "  "
            };
            let suffix = if self.candidates[idx].is_dir { "/" } else { "" };
            let text = format!("{}{}{}", marker, self.candidates[idx].path, suffix);
            let (line, _) = truncate_to_width(&text, inner_w);
            surface.put_str(x + 1, list_top + row, line, style);
        }

        if let Some(err) = &self.error_message {
            let (msg, _) = truncate_to_width(err, inner_w);
            surface.put_str(x + 1, y + popup_h - 2, msg, &error);
        }

        let before_cursor = &self.input.text[..self.input.byte_index_at_cursor()];
        let cursor_display = display_width(before_cursor).min(input_room.saturating_sub(1));
        let cursor_x = (input_x + cursor_display) as u16;
        let cursor_y = input_row as u16;
        Some((cursor_x, cursor_y))
    }

    pub fn input(&self) -> &str {
        &self.input.text
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn ctrl(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    #[test]
    fn enter_on_default_input_dispatches_save_as() {
        let tmp = tempdir().unwrap();
        let root = tmp.path().to_path_buf();
        let default = root.join("note.txt").to_string_lossy().to_string();

        let mut popup = SaveAsPopup::new(default.clone(), root);
        let result = popup.handle_key(key(KeyCode::Enter));

        match result {
            EventResult::Action(Action::App(AppAction::Buffer(BufferAction::SaveBufferAs(
                path,
            )))) => {
                assert_eq!(path, default);
            }
            _ => panic!("expected SaveBufferAs action"),
        }
    }

    #[test]
    fn relative_input_is_accepted() {
        let tmp = tempdir().unwrap();
        let root = tmp.path().to_path_buf();

        let mut popup = SaveAsPopup::new(String::new(), root);
        popup.input.set_text("notes/today.md".to_string());
        popup.refresh_candidates();
        let result = popup.handle_key(key(KeyCode::Enter));

        match result {
            EventResult::Action(Action::App(AppAction::Buffer(BufferAction::SaveBufferAs(
                path,
            )))) => {
                assert_eq!(path, "notes/today.md");
            }
            _ => panic!("expected SaveBufferAs action"),
        }
    }

    #[test]
    fn tab_completes_directory_with_trailing_slash() {
        let tmp = tempdir().unwrap();
        let root = tmp.path().to_path_buf();
        std::fs::create_dir_all(root.join("docs")).unwrap();
        std::fs::write(root.join("readme.md"), "ok").unwrap();

        let mut popup = SaveAsPopup::new(root.join("d").to_string_lossy().to_string(), root);
        popup.select_next();
        popup.select_prev();
        popup.complete_selected();
        assert!(popup.input.text.ends_with('/'));
    }

    #[test]
    fn ctrl_q_closes_popup() {
        let tmp = tempdir().unwrap();
        let root = tmp.path().to_path_buf();
        let mut popup = SaveAsPopup::new(String::new(), root);

        let result = popup.handle_key(ctrl('q'));
        assert!(matches!(
            result,
            EventResult::Action(Action::Ui(UiAction::CloseSaveAsPopup))
        ));
    }
}
