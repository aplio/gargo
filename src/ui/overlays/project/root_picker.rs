use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::input::action::{Action, AppAction, ProjectAction, UiAction};
use crate::ui::framework::cell::CellStyle;
use crate::ui::framework::component::EventResult;
use crate::ui::framework::surface::Surface;
use crate::ui::shared::filtering::fuzzy_match;
use crate::ui::text_input::TextInput;
use crate::ui::text::{display_width, truncate_to_width};

const MIN_POPUP_WIDTH: usize = 24;
const MIN_POPUP_HEIGHT: usize = 8;
const MAX_CANDIDATES: usize = 200;

#[derive(Clone)]
struct RootCandidate {
    path: String,
    name: String,
    score: i32,
}

pub struct ProjectRootPopup {
    input: TextInput,
    candidates: Vec<RootCandidate>,
    selected: usize,
    selection_active: bool,
    error_message: Option<String>,
}

impl ProjectRootPopup {
    pub fn new(current_root: PathBuf) -> Self {
        let mut popup = Self {
            input: TextInput::with_text(&current_root.to_string_lossy()),
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

    fn parse_parent_and_query(input: &str) -> Option<(PathBuf, String)> {
        if input.is_empty() {
            return None;
        }
        let input_path = Path::new(input);
        if !input_path.is_absolute() {
            return None;
        }

        if input.ends_with('/') {
            return Some((input_path.to_path_buf(), String::new()));
        }

        let parent = input_path.parent()?.to_path_buf();
        let query = input_path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_default();
        Some((parent, query))
    }

    fn refresh_candidates(&mut self) {
        self.candidates.clear();
        self.selected = 0;

        let Some((parent, query)) = Self::parse_parent_and_query(&self.input.text) else {
            return;
        };
        if !parent.is_dir() {
            return;
        }

        let mut collected = Vec::new();
        if let Ok(entries) = std::fs::read_dir(parent) {
            for entry in entries.flatten() {
                let Ok(ft) = entry.file_type() else {
                    continue;
                };
                if !ft.is_dir() {
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

                collected.push(RootCandidate {
                    path: entry.path().to_string_lossy().to_string(),
                    name,
                    score,
                });
            }
        }

        collected.sort_by(|a, b| {
            b.score
                .cmp(&a.score)
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
        if !completed.ends_with('/') {
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

    fn validate_directory(path: &str) -> Result<String, String> {
        let trimmed = path.trim();
        if trimmed.is_empty() {
            return Err("Path is empty".to_string());
        }

        let p = Path::new(trimmed);
        if !p.is_absolute() {
            return Err("Path must be absolute".to_string());
        }
        if !p.exists() {
            return Err("Path does not exist".to_string());
        }
        if !p.is_dir() {
            return Err("Path is not a directory".to_string());
        }

        let canonical = std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
        Ok(canonical.to_string_lossy().to_string())
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> EventResult {
        if key.kind != KeyEventKind::Press {
            return EventResult::Consumed;
        }

        if key.modifiers.contains(KeyModifiers::CONTROL) {
            return match key.code {
                KeyCode::Char('q') | KeyCode::Char('c') => {
                    EventResult::Action(Action::Ui(UiAction::CloseProjectRootPopup))
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
            KeyCode::Esc => EventResult::Action(Action::Ui(UiAction::CloseProjectRootPopup)),
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
                match Self::validate_directory(&target) {
                    Ok(path) => EventResult::Action(Action::App(AppAction::Project(
                        ProjectAction::ChangeProjectRoot(path),
                    ))),
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

        surface.put_str(x + 2, y, " Change Project Root ", &base);

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
            let text = format!("{}{}", marker, self.candidates[idx].path);
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
    fn parse_parent_and_query_works_for_file_like_and_dir_like_paths() {
        let (parent, query) = ProjectRootPopup::parse_parent_and_query("/a/b/c").unwrap();
        assert_eq!(parent, PathBuf::from("/a/b"));
        assert_eq!(query, "c");

        let (parent2, query2) = ProjectRootPopup::parse_parent_and_query("/a/b/c/").unwrap();
        assert_eq!(parent2, PathBuf::from("/a/b/c"));
        assert!(query2.is_empty());
    }

    #[test]
    fn ctrl_w_deletes_previous_segment() {
        let mut popup = ProjectRootPopup {
            input: TextInput::with_text("/tmp/foo/bar"),
            candidates: Vec::new(),
            selected: 0,
            selection_active: false,
            error_message: None,
        };
        popup.delete_prev_segment();
        assert_eq!(popup.input.text, "/tmp/foo/");
    }

    #[test]
    fn tab_completes_selected_candidate_and_enter_dispatches() {
        let tmp = tempdir().unwrap();
        let workspace = tmp.path().join("workspace");
        let repo_a = workspace.join("repo_a");
        let repo_b = workspace.join("repo_b");
        std::fs::create_dir_all(&repo_a).unwrap();
        std::fs::create_dir_all(&repo_b).unwrap();

        let mut popup = ProjectRootPopup::new(repo_a.clone());
        popup.handle_key(ctrl('w'));
        popup.handle_key(ctrl('n'));
        let _ = popup.handle_key(key(KeyCode::Tab));

        let result = popup.handle_key(key(KeyCode::Enter));
        match result {
            EventResult::Action(Action::App(AppAction::Project(
                ProjectAction::ChangeProjectRoot(path),
            ))) => {
                assert_eq!(PathBuf::from(path), std::fs::canonicalize(&repo_b).unwrap());
            }
            _ => panic!("expected ChangeProjectRoot action"),
        }
    }

    #[test]
    fn enter_invalid_absolute_path_keeps_popup_open() {
        let tmp = tempdir().unwrap();
        let mut popup = ProjectRootPopup::new(tmp.path().to_path_buf());
        popup
            .input
            .set_text("/this/path/does/not/exist".to_string());
        popup.refresh_candidates();

        let result = popup.handle_key(key(KeyCode::Enter));
        assert!(matches!(result, EventResult::Consumed));
        assert!(popup.error_message.is_some());
    }
}
