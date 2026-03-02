use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::command::recent_projects::RecentProjectEntry;
use crate::input::action::{Action, AppAction, ProjectAction, UiAction};
use crate::ui::framework::cell::CellStyle;
use crate::ui::framework::component::EventResult;
use crate::ui::framework::surface::Surface;
use crate::ui::shared::filtering::fzf_style_match;
use crate::ui::text_input::TextInput;
use crate::ui::text::{display_width, truncate_to_width};

const MIN_POPUP_WIDTH: usize = 24;
const MIN_POPUP_HEIGHT: usize = 8;

#[derive(Clone)]
struct RecentProjectCandidate {
    index: usize,
    score: i32,
}

pub struct RecentProjectPopup {
    input: TextInput,
    entries: Vec<RecentProjectEntry>,
    candidates: Vec<RecentProjectCandidate>,
    selected: usize,
}

impl RecentProjectPopup {
    pub fn new(entries: Vec<RecentProjectEntry>) -> Self {
        let mut popup = Self {
            input: TextInput::default(),
            entries,
            candidates: Vec::new(),
            selected: 0,
        };
        popup.refresh_candidates();
        popup
    }

    fn refresh_candidates(&mut self) {
        self.candidates = if self.input.text.trim().is_empty() {
            self.entries
                .iter()
                .enumerate()
                .map(|(index, _)| RecentProjectCandidate { index, score: 0 })
                .collect()
        } else {
            let mut filtered: Vec<RecentProjectCandidate> = self
                .entries
                .iter()
                .enumerate()
                .filter_map(|(index, entry)| {
                    fzf_style_match(&entry.project_path, &self.input.text)
                        .map(|(score, _)| RecentProjectCandidate { index, score })
                })
                .collect();
            filtered.sort_by(|a, b| {
                b.score.cmp(&a.score).then_with(|| {
                    self.entries[a.index]
                        .project_path
                        .cmp(&self.entries[b.index].project_path)
                })
            });
            filtered
        };
        if self.selected >= self.candidates.len() {
            self.selected = self.candidates.len().saturating_sub(1);
        }
    }

    fn select_next(&mut self) {
        if self.candidates.is_empty() {
            return;
        }
        self.selected = (self.selected + 1) % self.candidates.len();
    }

    fn select_prev(&mut self) {
        if self.candidates.is_empty() {
            return;
        }
        self.selected = if self.selected == 0 {
            self.candidates.len() - 1
        } else {
            self.selected - 1
        };
    }

    fn selected_project_path(&self) -> Option<String> {
        let candidate = self.candidates.get(self.selected)?;
        Some(self.entries[candidate.index].project_path.clone())
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> EventResult {
        if key.kind != KeyEventKind::Press {
            return EventResult::Consumed;
        }

        if key.modifiers.contains(KeyModifiers::CONTROL) {
            return match key.code {
                KeyCode::Char('q') | KeyCode::Char('c') => {
                    EventResult::Action(Action::Ui(UiAction::CloseRecentProjectPopup))
                }
                KeyCode::Char('n') | KeyCode::Char('j') | KeyCode::Down => {
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
                    if self.input.delete_prev_word() {
                        self.refresh_candidates();
                    }
                    EventResult::Consumed
                }
                KeyCode::Char('k') => {
                    if self.input.delete_to_end() {
                        self.refresh_candidates();
                    }
                    EventResult::Consumed
                }
                _ => EventResult::Consumed,
            };
        }

        match key.code {
            KeyCode::Esc => EventResult::Action(Action::Ui(UiAction::CloseRecentProjectPopup)),
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
                    self.refresh_candidates();
                }
                EventResult::Consumed
            }
            KeyCode::Enter => {
                if let Some(project_path) = self.selected_project_path() {
                    EventResult::Action(Action::App(AppAction::Project(
                        ProjectAction::SwitchToRecentProject(project_path),
                    )))
                } else {
                    EventResult::Action(Action::Ui(UiAction::CloseRecentProjectPopup))
                }
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::ALT) => {
                self.input.insert_char(c);
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

        surface.put_str(x + 2, y, " Switch to Recent Project ", &base);

        let input_row = y + 1;
        let prompt = "query: ";
        surface.put_str(x + 1, input_row, prompt, &base);
        let input_x = x + 1 + display_width(prompt);
        let input_room = inner_w.saturating_sub(display_width(prompt));
        let (truncated_input, _) = truncate_to_width(&self.input.text, input_room);
        surface.put_str(input_x, input_row, truncated_input, &base);

        let hint_row = y + 2;
        let hint = "ctrl-n/p or up/down select  enter switch  esc close";
        let (hint_text, _) = truncate_to_width(hint, inner_w);
        surface.put_str(x + 1, hint_row, hint_text, &dim);

        let list_top = y + 3;
        let list_bottom = y + popup_h - 1;
        let list_h = list_bottom.saturating_sub(list_top);
        let mut start = 0usize;
        if self.selected >= list_h && list_h > 0 {
            start = self.selected + 1 - list_h;
        }

        for row in 0..list_h {
            let idx = start + row;
            if idx >= self.candidates.len() {
                break;
            }
            let candidate = &self.candidates[idx];
            let entry = &self.entries[candidate.index];
            let style = if idx == self.selected {
                &selected
            } else {
                &base
            };
            let marker = if idx == self.selected { "> " } else { "  " };
            let label = if let Some(rel) = &entry.last_open_file {
                format!("{}{} [{}]", marker, entry.project_path, rel)
            } else {
                format!("{}{}", marker, entry.project_path)
            };
            let (line, _) = truncate_to_width(&label, inner_w);
            surface.put_str(x + 1, list_top + row, line, style);
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

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn ctrl(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    fn entry(path: &str) -> RecentProjectEntry {
        RecentProjectEntry {
            project_path: path.to_string(),
            last_open_at: 0,
            last_edit_at: 0,
            last_open_file: None,
            last_edit_file: None,
        }
    }

    #[test]
    fn ctrl_n_and_ctrl_p_move_selection() {
        let mut popup = RecentProjectPopup::new(vec![entry("/tmp/a"), entry("/tmp/b")]);
        assert_eq!(popup.selected, 0);

        popup.handle_key(ctrl('n'));
        assert_eq!(popup.selected, 1);

        popup.handle_key(ctrl('p'));
        assert_eq!(popup.selected, 0);
    }

    #[test]
    fn enter_dispatches_switch_action() {
        let mut popup = RecentProjectPopup::new(vec![entry("/tmp/repo")]);
        let result = popup.handle_key(key(KeyCode::Enter));
        assert_eq!(
            result,
            EventResult::Action(Action::App(AppAction::Project(
                ProjectAction::SwitchToRecentProject("/tmp/repo".to_string())
            )))
        );
    }

    #[test]
    fn typing_filters_candidates_with_fzf_style_match() {
        let mut popup =
            RecentProjectPopup::new(vec![entry("/tmp/gargo2"), entry("/tmp/another_repo")]);
        popup.handle_key(key(KeyCode::Char('g')));
        popup.handle_key(key(KeyCode::Char('2')));
        assert_eq!(popup.candidates.len(), 1);
        assert_eq!(
            popup.entries[popup.candidates[0].index].project_path,
            "/tmp/gargo2"
        );
    }
}
