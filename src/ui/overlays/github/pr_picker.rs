use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEventKind};
use crossterm::style::Color;
use serde::Deserialize;

use crate::input::action::{Action, AppAction, IntegrationAction, UiAction};
use crate::ui::framework::cell::CellStyle;
use crate::ui::framework::component::EventResult;
use crate::ui::framework::surface::Surface;
use crate::ui::shared::filtering::fuzzy_match;
use crate::ui::text_input::delete_prev_word_input;
use crate::ui::text::{display_width, slice_display_window, truncate_to_width};

#[derive(Debug, Deserialize)]
struct GhAuthor {
    login: String,
}

#[derive(Debug, Deserialize)]
struct GhLabel {
    name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GhPrEntry {
    number: u64,
    title: String,
    body: String,
    url: String,
    state: String,
    author: GhAuthor,
    head_ref_name: String,
    created_at: String,
    #[serde(default)]
    labels: Vec<GhLabel>,
}

#[derive(Debug, Clone)]
pub struct PrEntry {
    pub number: u64,
    pub title: String,
    pub body: String,
    pub url: String,
    pub state: String,
    pub author: String,
    pub head_ref: String,
    pub created_at: String,
    pub labels: Vec<String>,
}

pub fn parse_gh_pr_json(json: &str) -> Result<Vec<PrEntry>, String> {
    let gh_entries: Vec<GhPrEntry> =
        serde_json::from_str(json).map_err(|e| format!("Failed to parse gh output: {}", e))?;
    Ok(gh_entries
        .into_iter()
        .map(|e| PrEntry {
            number: e.number,
            title: e.title,
            body: e.body,
            url: e.url,
            state: e.state,
            author: e.author.login,
            head_ref: e.head_ref_name,
            created_at: e.created_at,
            labels: e.labels.into_iter().map(|l| l.name).collect(),
        })
        .collect())
}

pub struct PrListPicker {
    entries: Vec<PrEntry>,
    filtered: Vec<usize>,
    selected: usize,
    scroll_offset: usize,
    find_active: bool,
    find_input: String,
    copy_menu_active: bool,
    preview_scroll: usize,
    preview_horizontal_scroll: usize,
    message: Option<String>,
}

const PREVIEW_SPLIT_THRESHOLD: usize = 60;
const MOUSE_SCROLL_LINES: usize = 3;
const HORIZONTAL_SCROLL_COLS: usize = 8;

impl PrListPicker {
    pub fn new(entries: Vec<PrEntry>) -> Self {
        let msg = if entries.is_empty() {
            Some("No PRs found".to_string())
        } else {
            None
        };
        let filtered: Vec<usize> = (0..entries.len()).collect();
        Self {
            entries,
            filtered,
            selected: 0,
            scroll_offset: 0,
            find_active: false,
            find_input: String::new(),
            copy_menu_active: false,
            preview_scroll: 0,
            preview_horizontal_scroll: 0,
            message: msg,
        }
    }

    fn selected_entry(&self) -> Option<&PrEntry> {
        self.filtered
            .get(self.selected)
            .and_then(|&idx| self.entries.get(idx))
    }

    fn update_filtered(&mut self) {
        if self.find_input.is_empty() {
            self.filtered = (0..self.entries.len()).collect();
        } else {
            let mut scored: Vec<(i32, usize)> = self
                .entries
                .iter()
                .enumerate()
                .filter_map(|(i, entry)| {
                    let haystack = format!("#{} {}", entry.number, entry.title);
                    fuzzy_match(&haystack, &self.find_input).map(|(score, _)| (score, i))
                })
                .collect();
            scored.sort_by(|a, b| b.0.cmp(&a.0));
            self.filtered = scored.into_iter().map(|(_, i)| i).collect();
        }
        if self.selected >= self.filtered.len() {
            self.selected = self.filtered.len().saturating_sub(1);
        }
        self.preview_scroll = 0;
        self.preview_horizontal_scroll = 0;
    }

    fn move_down(&mut self) {
        if self.filtered.is_empty() {
            return;
        }
        if self.selected + 1 < self.filtered.len() {
            self.selected += 1;
            self.preview_scroll = 0;
            self.preview_horizontal_scroll = 0;
        }
    }

    fn move_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
            self.preview_scroll = 0;
            self.preview_horizontal_scroll = 0;
        }
    }

    fn preview_max_scroll(&self, preview_len: usize, content_h: usize) -> usize {
        if content_h == 0 {
            0
        } else {
            preview_len.saturating_sub(content_h)
        }
    }

    fn clamp_preview_scroll(&mut self, preview_len: usize, content_h: usize) {
        let max_scroll = self.preview_max_scroll(preview_len, content_h);
        if self.preview_scroll > max_scroll {
            self.preview_scroll = max_scroll;
        }
    }

    fn scroll_preview_down_lines(&mut self, lines: usize, content_h: usize) {
        let preview_len = self.preview_lines().len();
        let max_scroll = self.preview_max_scroll(preview_len, content_h);
        self.preview_scroll = self.preview_scroll.saturating_add(lines).min(max_scroll);
    }

    fn scroll_preview_up_lines(&mut self, lines: usize) {
        self.preview_scroll = self.preview_scroll.saturating_sub(lines);
    }

    fn scroll_preview_down(&mut self) {
        self.preview_scroll = self.preview_scroll.saturating_add(1);
    }

    fn scroll_preview_up(&mut self) {
        self.scroll_preview_up_lines(1);
    }

    fn preview_max_horizontal_scroll(
        &self,
        preview: &[(String, CellStyle)],
        content_w: usize,
    ) -> usize {
        if content_w == 0 {
            return 0;
        }
        preview
            .iter()
            .map(|(line, _)| display_width(line).saturating_sub(content_w))
            .max()
            .unwrap_or(0)
    }

    fn clamp_preview_horizontal_scroll(
        &mut self,
        preview: &[(String, CellStyle)],
        content_w: usize,
    ) {
        let max_scroll = self.preview_max_horizontal_scroll(preview, content_w);
        if self.preview_horizontal_scroll > max_scroll {
            self.preview_horizontal_scroll = max_scroll;
        }
    }

    fn scroll_preview_right(&mut self) {
        self.preview_horizontal_scroll = self
            .preview_horizontal_scroll
            .saturating_add(HORIZONTAL_SCROLL_COLS);
    }

    fn scroll_preview_left(&mut self) {
        self.preview_horizontal_scroll = self
            .preview_horizontal_scroll
            .saturating_sub(HORIZONTAL_SCROLL_COLS);
    }

    fn popup_size(cols: usize, rows: usize) -> (usize, usize) {
        ((cols * 80 / 100).max(3), (rows * 80 / 100).max(3))
    }

    fn preview_content_height_for_surface(cols: usize, rows: usize) -> Option<usize> {
        let (popup_w, popup_h) = Self::popup_size(cols, rows);
        if popup_w >= PREVIEW_SPLIT_THRESHOLD {
            Some(popup_h.saturating_sub(2))
        } else {
            None
        }
    }

    pub fn handle_mouse_scroll(
        &mut self,
        kind: MouseEventKind,
        cols: usize,
        rows: usize,
    ) -> EventResult {
        let Some(content_h) = Self::preview_content_height_for_surface(cols, rows) else {
            return EventResult::Ignored;
        };
        match kind {
            MouseEventKind::ScrollDown => {
                self.scroll_preview_down_lines(MOUSE_SCROLL_LINES, content_h);
                EventResult::Consumed
            }
            MouseEventKind::ScrollUp => {
                self.scroll_preview_up_lines(MOUSE_SCROLL_LINES);
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }

    fn preview_lines(&self) -> Vec<(String, CellStyle)> {
        let Some(entry) = self.selected_entry() else {
            return vec![(
                "No PR selected".to_string(),
                CellStyle {
                    dim: true,
                    ..CellStyle::default()
                },
            )];
        };

        let mut lines: Vec<(String, CellStyle)> = Vec::new();

        let state_style = match entry.state.as_str() {
            "OPEN" => CellStyle {
                fg: Some(Color::Green),
                bold: true,
                ..CellStyle::default()
            },
            "CLOSED" => CellStyle {
                fg: Some(Color::Red),
                bold: true,
                ..CellStyle::default()
            },
            "MERGED" => CellStyle {
                fg: Some(Color::Magenta),
                bold: true,
                ..CellStyle::default()
            },
            _ => CellStyle {
                bold: true,
                ..CellStyle::default()
            },
        };
        lines.push((
            format!("[{}] by {}", entry.state, entry.author),
            state_style,
        ));

        lines.push((
            format!("Branch: {}", entry.head_ref),
            CellStyle {
                fg: Some(Color::Cyan),
                ..CellStyle::default()
            },
        ));

        if !entry.labels.is_empty() {
            lines.push((
                format!("Labels: {}", entry.labels.join(", ")),
                CellStyle {
                    fg: Some(Color::Yellow),
                    ..CellStyle::default()
                },
            ));
        }

        let date_display = entry
            .created_at
            .split('T')
            .next()
            .unwrap_or(&entry.created_at);
        lines.push((
            format!("Created: {}", date_display),
            CellStyle {
                dim: true,
                ..CellStyle::default()
            },
        ));

        lines.push((String::new(), CellStyle::default()));

        let body = if entry.body.is_empty() {
            "(no description)"
        } else {
            &entry.body
        };
        let body_style = CellStyle::default();
        for line in body.lines() {
            lines.push((line.to_string(), body_style));
        }

        lines
    }

    fn delete_prev_word(&mut self) {
        delete_prev_word_input(&mut self.find_input);
    }

    fn handle_find_key(&mut self, key: KeyEvent) -> EventResult {
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            return match key.code {
                KeyCode::Char('q') => EventResult::Action(Action::Ui(UiAction::ClosePrListPicker)),
                KeyCode::Char('n') => {
                    self.move_down();
                    EventResult::Consumed
                }
                KeyCode::Char('p') => {
                    self.move_up();
                    EventResult::Consumed
                }
                KeyCode::Char('f') => {
                    self.scroll_preview_down();
                    EventResult::Consumed
                }
                KeyCode::Char('b') => {
                    self.scroll_preview_up();
                    EventResult::Consumed
                }
                KeyCode::Char('w') => {
                    self.delete_prev_word();
                    self.update_filtered();
                    EventResult::Consumed
                }
                KeyCode::Char('k') | KeyCode::Char('u') => {
                    self.find_input.clear();
                    self.update_filtered();
                    EventResult::Consumed
                }
                _ => EventResult::Consumed,
            };
        }

        match key.code {
            KeyCode::Esc => {
                self.find_active = false;
                self.find_input.clear();
                self.update_filtered();
                EventResult::Consumed
            }
            KeyCode::Enter => {
                self.find_active = false;
                EventResult::Consumed
            }
            KeyCode::Backspace => {
                self.find_input.pop();
                self.update_filtered();
                EventResult::Consumed
            }
            KeyCode::Up => {
                self.move_up();
                EventResult::Consumed
            }
            KeyCode::Down => {
                self.move_down();
                EventResult::Consumed
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::ALT) => {
                self.find_input.push(c);
                self.update_filtered();
                EventResult::Consumed
            }
            _ => EventResult::Consumed,
        }
    }

    fn handle_copy_menu_key(&mut self, key: KeyEvent) -> EventResult {
        self.copy_menu_active = false;
        let Some(entry) = self.selected_entry() else {
            return EventResult::Consumed;
        };
        match key.code {
            KeyCode::Char('u') => EventResult::Action(Action::App(AppAction::Integration(
                IntegrationAction::CopyToClipboard {
                    text: entry.url.clone(),
                    description: "PR URL".to_string(),
                },
            ))),
            KeyCode::Char('n') => EventResult::Action(Action::App(AppAction::Integration(
                IntegrationAction::CopyToClipboard {
                    text: format!("#{}", entry.number),
                    description: "PR number".to_string(),
                },
            ))),
            KeyCode::Char('t') => EventResult::Action(Action::App(AppAction::Integration(
                IntegrationAction::CopyToClipboard {
                    text: entry.title.clone(),
                    description: "PR title".to_string(),
                },
            ))),
            _ => EventResult::Consumed,
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> EventResult {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('q') {
            return EventResult::Action(Action::Ui(UiAction::ClosePrListPicker));
        }

        if self.find_active {
            return self.handle_find_key(key);
        }

        if self.copy_menu_active {
            return self.handle_copy_menu_key(key);
        }

        let has_shift = key.modifiers.contains(KeyModifiers::SHIFT);

        if key.modifiers.contains(KeyModifiers::CONTROL) {
            return match key.code {
                KeyCode::Char('n') => {
                    if has_shift {
                        self.scroll_preview_down();
                    } else {
                        self.move_down();
                    }
                    EventResult::Consumed
                }
                KeyCode::Char('p') => {
                    if has_shift {
                        self.scroll_preview_up();
                    } else {
                        self.move_up();
                    }
                    EventResult::Consumed
                }
                _ => EventResult::Consumed,
            };
        }

        if has_shift {
            return match key.code {
                KeyCode::Char('J') | KeyCode::Down => {
                    self.scroll_preview_down();
                    EventResult::Consumed
                }
                KeyCode::Char('K') | KeyCode::Up => {
                    self.scroll_preview_up();
                    EventResult::Consumed
                }
                KeyCode::Char('L') | KeyCode::Right => {
                    self.scroll_preview_right();
                    EventResult::Consumed
                }
                KeyCode::Char('H') | KeyCode::Left => {
                    self.scroll_preview_left();
                    EventResult::Consumed
                }
                _ => EventResult::Consumed,
            };
        }

        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                self.move_down();
                EventResult::Consumed
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.move_up();
                EventResult::Consumed
            }
            KeyCode::Char('/') => {
                self.find_active = true;
                self.find_input.clear();
                EventResult::Consumed
            }
            KeyCode::Char('o') | KeyCode::Enter => {
                if let Some(entry) = self.selected_entry() {
                    let url = entry.url.clone();
                    EventResult::Action(Action::App(AppAction::Integration(
                        IntegrationAction::OpenPrUrl(url),
                    )))
                } else {
                    EventResult::Consumed
                }
            }
            KeyCode::Char('c') => {
                self.copy_menu_active = true;
                EventResult::Consumed
            }
            KeyCode::Esc | KeyCode::Char('q') => {
                EventResult::Action(Action::Ui(UiAction::ClosePrListPicker))
            }
            _ => EventResult::Consumed,
        }
    }

    pub fn render_overlay(&mut self, surface: &mut Surface) -> Option<(u16, u16)> {
        let cols = surface.width;
        let rows = surface.height;
        let (popup_w, popup_h) = Self::popup_size(cols, rows);
        let offset_x = (cols.saturating_sub(popup_w)) / 2;
        let offset_y = (rows.saturating_sub(popup_h)) / 2;

        if popup_w >= PREVIEW_SPLIT_THRESHOLD {
            let gap = 2;
            let left_w = (popup_w - gap) / 2;
            let right_w = popup_w - gap - left_w;
            let right_x = offset_x + left_w + gap;

            let cursor = self.render_pr_panel(surface, offset_x, offset_y, left_w, popup_h);
            self.render_preview_panel(surface, right_x, offset_y, right_w, popup_h);
            cursor
        } else {
            self.render_pr_panel(surface, offset_x, offset_y, popup_w, popup_h)
        }
    }

    fn render_pr_panel(
        &mut self,
        surface: &mut Surface,
        x: usize,
        y: usize,
        w: usize,
        h: usize,
    ) -> Option<(u16, u16)> {
        let inner_w = w.saturating_sub(2);
        let default_style = CellStyle::default();

        // Layout:
        // row 0: top border
        // row 1: title row
        // row 2..h-2: PR list
        // row h-2: status/hint row
        // row h-1: bottom border

        let content_start = 2;
        let content_end = h.saturating_sub(2);
        let content_h = content_end.saturating_sub(content_start);

        // Adjust scroll_offset to keep selected visible
        if self.selected < self.scroll_offset {
            self.scroll_offset = self.selected;
        }
        if self.selected >= self.scroll_offset + content_h {
            self.scroll_offset = self.selected.saturating_sub(content_h.saturating_sub(1));
        }

        for row in 0..h {
            if row == 0 {
                surface.put_str(x, y + row, "\u{250c}", &default_style);
                surface.fill_region(x + 1, y + row, inner_w, '\u{2500}', &default_style);
                surface.put_str(x + 1 + inner_w, y + row, "\u{2510}", &default_style);
            } else if row == h - 1 {
                surface.put_str(x, y + row, "\u{2514}", &default_style);
                surface.fill_region(x + 1, y + row, inner_w, '\u{2500}', &default_style);
                surface.put_str(x + 1 + inner_w, y + row, "\u{2518}", &default_style);
            } else if row == 1 {
                // Title row
                surface.put_str(x, y + row, "\u{2502}", &default_style);
                let title = format!(" Pull Requests ({})", self.filtered.len());
                let title_style = CellStyle {
                    bold: true,
                    fg: Some(Color::Cyan),
                    ..CellStyle::default()
                };
                let (truncated, used) = truncate_to_width(&title, inner_w);
                surface.put_str(x + 1, y + row, truncated, &title_style);
                if used < inner_w {
                    surface.fill_region(x + 1 + used, y + row, inner_w - used, ' ', &default_style);
                }
                surface.put_str(x + 1 + inner_w, y + row, "\u{2502}", &default_style);
            } else if row == h - 2 {
                // Status/hint row
                surface.put_str(x, y + row, "\u{2502}", &default_style);
                let row_style = CellStyle {
                    reverse: true,
                    ..CellStyle::default()
                };
                if self.find_active {
                    let prompt = format!("/{}", self.find_input);
                    let (truncated, used) = truncate_to_width(&prompt, inner_w);
                    surface.put_str(x + 1, y + row, truncated, &row_style);
                    if used < inner_w {
                        surface.fill_region(x + 1 + used, y + row, inner_w - used, ' ', &row_style);
                    }
                } else if self.copy_menu_active {
                    let hint = "u:url n:number t:title";
                    let (truncated, used) = truncate_to_width(hint, inner_w);
                    surface.put_str(x + 1, y + row, truncated, &row_style);
                    if used < inner_w {
                        surface.fill_region(x + 1 + used, y + row, inner_w - used, ' ', &row_style);
                    }
                } else if let Some(ref msg) = self.message {
                    let (truncated, used) = truncate_to_width(msg, inner_w);
                    surface.put_str(x + 1, y + row, truncated, &row_style);
                    if used < inner_w {
                        surface.fill_region(x + 1 + used, y + row, inner_w - used, ' ', &row_style);
                    }
                } else {
                    let hint = "o:open c:copy /:find q:close";
                    let dim_style = CellStyle {
                        dim: true,
                        reverse: true,
                        ..CellStyle::default()
                    };
                    let (truncated, used) = truncate_to_width(hint, inner_w);
                    surface.put_str(x + 1, y + row, truncated, &dim_style);
                    if used < inner_w {
                        surface.fill_region(x + 1 + used, y + row, inner_w - used, ' ', &dim_style);
                    }
                }
                surface.put_str(x + 1 + inner_w, y + row, "\u{2502}", &default_style);
            } else if content_h > 0 {
                // Content rows
                let content_row = row - content_start;
                let item_idx = self.scroll_offset + content_row;

                surface.put_str(x, y + row, "\u{2502}", &default_style);

                if item_idx < self.filtered.len() {
                    let entry = &self.entries[self.filtered[item_idx]];
                    let is_selected = item_idx == self.selected;
                    let label = format!(" #{} {}", entry.number, entry.title);

                    let style = if is_selected {
                        CellStyle {
                            reverse: true,
                            ..CellStyle::default()
                        }
                    } else {
                        CellStyle::default()
                    };
                    let (truncated, used) = truncate_to_width(&label, inner_w);
                    surface.put_str(x + 1, y + row, truncated, &style);
                    if used < inner_w {
                        surface.fill_region(x + 1 + used, y + row, inner_w - used, ' ', &style);
                    }
                } else {
                    surface.fill_region(x + 1, y + row, inner_w, ' ', &default_style);
                }

                surface.put_str(x + 1 + inner_w, y + row, "\u{2502}", &default_style);
            } else {
                surface.put_str(x, y + row, "\u{2502}", &default_style);
                surface.fill_region(x + 1, y + row, inner_w, ' ', &default_style);
                surface.put_str(x + 1 + inner_w, y + row, "\u{2502}", &default_style);
            }
        }

        if self.find_active {
            let find_row = y + h.saturating_sub(2);
            let prompt = format!("/{}", self.find_input);
            let (_, used) = truncate_to_width(&prompt, inner_w);
            let cursor_x = (x + 1 + used) as u16;
            let cursor_y = find_row as u16;
            Some((cursor_x, cursor_y))
        } else {
            None
        }
    }

    fn render_preview_panel(
        &mut self,
        surface: &mut Surface,
        x: usize,
        y: usize,
        w: usize,
        h: usize,
    ) {
        let inner_w = w.saturating_sub(2);
        let content_h = h.saturating_sub(2);
        let default_style = CellStyle::default();
        let preview = self.preview_lines();
        self.clamp_preview_scroll(preview.len(), content_h);
        self.clamp_preview_horizontal_scroll(&preview, inner_w);

        for row in 0..h {
            if row == 0 {
                surface.put_str(x, y + row, "\u{250c}", &default_style);
                surface.fill_region(x + 1, y + row, inner_w, '\u{2500}', &default_style);
                surface.put_str(x + 1 + inner_w, y + row, "\u{2510}", &default_style);
            } else if row == h - 1 {
                surface.put_str(x, y + row, "\u{2514}", &default_style);
                surface.fill_region(x + 1, y + row, inner_w, '\u{2500}', &default_style);
                surface.put_str(x + 1 + inner_w, y + row, "\u{2518}", &default_style);
            } else {
                let line_idx = self.preview_scroll + (row - 1);
                surface.put_str(x, y + row, "\u{2502}", &default_style);
                if line_idx < preview.len() && (row - 1) < content_h {
                    let (text, style) = &preview[line_idx];
                    let window =
                        slice_display_window(text, self.preview_horizontal_scroll, inner_w);
                    surface.put_str(x + 1, y + row, window.visible, style);
                    if window.used_width < inner_w {
                        surface.fill_region(
                            x + 1 + window.used_width,
                            y + row,
                            inner_w - window.used_width,
                            ' ',
                            &default_style,
                        );
                    }
                } else {
                    surface.fill_region(x + 1, y + row, inner_w, ' ', &default_style);
                }
                surface.put_str(x + 1 + inner_w, y + row, "\u{2502}", &default_style);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn ctrl_key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    fn shift_key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::SHIFT)
    }

    fn test_entries() -> Vec<PrEntry> {
        vec![
            PrEntry {
                number: 42,
                title: "Add dark mode support".to_string(),
                body: "This PR adds dark mode.\n\nDetails here.".to_string(),
                url: "https://github.com/user/repo/pull/42".to_string(),
                state: "OPEN".to_string(),
                author: "alice".to_string(),
                head_ref: "feature/dark-mode".to_string(),
                created_at: "2025-01-15T10:30:00Z".to_string(),
                labels: vec!["enhancement".to_string()],
            },
            PrEntry {
                number: 41,
                title: "Fix crash on startup".to_string(),
                body: "Fixes #40".to_string(),
                url: "https://github.com/user/repo/pull/41".to_string(),
                state: "MERGED".to_string(),
                author: "bob".to_string(),
                head_ref: "fix/startup-crash".to_string(),
                created_at: "2025-01-14T08:00:00Z".to_string(),
                labels: vec![],
            },
            PrEntry {
                number: 40,
                title: "Update dependencies".to_string(),
                body: "".to_string(),
                url: "https://github.com/user/repo/pull/40".to_string(),
                state: "CLOSED".to_string(),
                author: "charlie".to_string(),
                head_ref: "chore/deps".to_string(),
                created_at: "2025-01-13T12:00:00Z".to_string(),
                labels: vec!["chore".to_string(), "deps".to_string()],
            },
        ]
    }

    fn test_picker() -> PrListPicker {
        PrListPicker::new(test_entries())
    }

    #[test]
    fn parse_valid_json() {
        let json = r#"[
            {
                "number": 1,
                "title": "Test PR",
                "body": "body text",
                "url": "https://github.com/x/y/pull/1",
                "state": "OPEN",
                "author": {"login": "user1"},
                "headRefName": "feat/test",
                "createdAt": "2025-01-01T00:00:00Z",
                "labels": [{"name": "bug"}]
            }
        ]"#;
        let entries = parse_gh_pr_json(json).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].number, 1);
        assert_eq!(entries[0].title, "Test PR");
        assert_eq!(entries[0].author, "user1");
        assert_eq!(entries[0].head_ref, "feat/test");
        assert_eq!(entries[0].labels, vec!["bug".to_string()]);
    }

    #[test]
    fn parse_empty_json() {
        let entries = parse_gh_pr_json("[]").unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn parse_invalid_json() {
        assert!(parse_gh_pr_json("not json").is_err());
    }

    #[test]
    fn navigation_down_and_up() {
        let mut picker = test_picker();
        assert_eq!(picker.selected, 0);
        picker.handle_key(key(KeyCode::Char('j')));
        assert_eq!(picker.selected, 1);
        picker.handle_key(key(KeyCode::Char('j')));
        assert_eq!(picker.selected, 2);
        // Should not go past the end
        picker.handle_key(key(KeyCode::Char('j')));
        assert_eq!(picker.selected, 2);
        picker.handle_key(key(KeyCode::Char('k')));
        assert_eq!(picker.selected, 1);
        picker.handle_key(key(KeyCode::Char('k')));
        assert_eq!(picker.selected, 0);
        // Should not go below 0
        picker.handle_key(key(KeyCode::Char('k')));
        assert_eq!(picker.selected, 0);
    }

    #[test]
    fn arrow_key_navigation() {
        let mut picker = test_picker();
        picker.handle_key(key(KeyCode::Down));
        assert_eq!(picker.selected, 1);
        picker.handle_key(key(KeyCode::Up));
        assert_eq!(picker.selected, 0);
    }

    #[test]
    fn find_mode_filters_entries() {
        let mut picker = test_picker();
        picker.handle_key(key(KeyCode::Char('/')));
        assert!(picker.find_active);
        picker.handle_key(key(KeyCode::Char('d')));
        picker.handle_key(key(KeyCode::Char('a')));
        picker.handle_key(key(KeyCode::Char('r')));
        picker.handle_key(key(KeyCode::Char('k')));
        // Should filter to "Add dark mode support"
        assert_eq!(picker.filtered.len(), 1);
        assert_eq!(picker.entries[picker.filtered[0]].number, 42);
    }

    #[test]
    fn find_mode_esc_exits_and_clears() {
        let mut picker = test_picker();
        picker.handle_key(key(KeyCode::Char('/')));
        picker.handle_key(key(KeyCode::Char('x')));
        assert!(picker.find_active);
        picker.handle_key(key(KeyCode::Esc));
        assert!(!picker.find_active);
        assert!(picker.find_input.is_empty());
        assert_eq!(picker.filtered.len(), 3);
    }

    #[test]
    fn find_mode_enter_exits_keeps_filter() {
        let mut picker = test_picker();
        picker.handle_key(key(KeyCode::Char('/')));
        picker.handle_key(key(KeyCode::Char('c')));
        picker.handle_key(key(KeyCode::Char('r')));
        picker.handle_key(key(KeyCode::Char('a')));
        picker.handle_key(key(KeyCode::Char('s')));
        picker.handle_key(key(KeyCode::Char('h')));
        let filtered_count = picker.filtered.len();
        picker.handle_key(key(KeyCode::Enter));
        assert!(!picker.find_active);
        assert_eq!(picker.filtered.len(), filtered_count);
    }

    #[test]
    fn find_mode_ctrl_w_deletes_word() {
        let mut picker = test_picker();
        picker.handle_key(key(KeyCode::Char('/')));
        for c in "hello world".chars() {
            picker.handle_key(key(KeyCode::Char(c)));
        }
        assert_eq!(picker.find_input, "hello world");
        picker.handle_key(ctrl_key('w'));
        assert_eq!(picker.find_input, "hello ");
    }

    #[test]
    fn open_produces_open_pr_url() {
        let mut picker = test_picker();
        let result = picker.handle_key(key(KeyCode::Char('o')));
        match result {
            EventResult::Action(Action::App(AppAction::Integration(
                IntegrationAction::OpenPrUrl(url),
            ))) => {
                assert_eq!(url, "https://github.com/user/repo/pull/42");
            }
            _ => panic!("Expected OpenPrUrl action"),
        }
    }

    #[test]
    fn enter_opens_pr() {
        let mut picker = test_picker();
        let result = picker.handle_key(key(KeyCode::Enter));
        match result {
            EventResult::Action(Action::App(AppAction::Integration(
                IntegrationAction::OpenPrUrl(url),
            ))) => {
                assert_eq!(url, "https://github.com/user/repo/pull/42");
            }
            _ => panic!("Expected OpenPrUrl action"),
        }
    }

    #[test]
    fn copy_menu_url() {
        let mut picker = test_picker();
        picker.handle_key(key(KeyCode::Char('c')));
        assert!(picker.copy_menu_active);
        let result = picker.handle_key(key(KeyCode::Char('u')));
        assert!(!picker.copy_menu_active);
        match result {
            EventResult::Action(Action::App(AppAction::Integration(
                IntegrationAction::CopyToClipboard { text, description },
            ))) => {
                assert_eq!(text, "https://github.com/user/repo/pull/42");
                assert_eq!(description, "PR URL");
            }
            _ => panic!("Expected CopyToClipboard action"),
        }
    }

    #[test]
    fn copy_menu_number() {
        let mut picker = test_picker();
        picker.handle_key(key(KeyCode::Char('c')));
        let result = picker.handle_key(key(KeyCode::Char('n')));
        match result {
            EventResult::Action(Action::App(AppAction::Integration(
                IntegrationAction::CopyToClipboard { text, description },
            ))) => {
                assert_eq!(text, "#42");
                assert_eq!(description, "PR number");
            }
            _ => panic!("Expected CopyToClipboard action"),
        }
    }

    #[test]
    fn copy_menu_title() {
        let mut picker = test_picker();
        picker.handle_key(key(KeyCode::Char('c')));
        let result = picker.handle_key(key(KeyCode::Char('t')));
        match result {
            EventResult::Action(Action::App(AppAction::Integration(
                IntegrationAction::CopyToClipboard { text, description },
            ))) => {
                assert_eq!(text, "Add dark mode support");
                assert_eq!(description, "PR title");
            }
            _ => panic!("Expected CopyToClipboard action"),
        }
    }

    #[test]
    fn copy_menu_cancel() {
        let mut picker = test_picker();
        picker.handle_key(key(KeyCode::Char('c')));
        assert!(picker.copy_menu_active);
        let result = picker.handle_key(key(KeyCode::Char('x')));
        assert!(!picker.copy_menu_active);
        assert!(matches!(result, EventResult::Consumed));
    }

    #[test]
    fn esc_closes() {
        let mut picker = test_picker();
        let result = picker.handle_key(key(KeyCode::Esc));
        assert!(matches!(
            result,
            EventResult::Action(Action::Ui(UiAction::ClosePrListPicker))
        ));
    }

    #[test]
    fn q_closes() {
        let mut picker = test_picker();
        let result = picker.handle_key(key(KeyCode::Char('q')));
        assert!(matches!(
            result,
            EventResult::Action(Action::Ui(UiAction::ClosePrListPicker))
        ));
    }

    #[test]
    fn ctrl_q_closes() {
        let mut picker = test_picker();
        let result = picker.handle_key(ctrl_key('q'));
        assert!(matches!(
            result,
            EventResult::Action(Action::Ui(UiAction::ClosePrListPicker))
        ));
    }

    #[test]
    fn empty_entries_shows_message() {
        let picker = PrListPicker::new(vec![]);
        assert_eq!(picker.message.as_deref(), Some("No PRs found"));
        assert!(picker.filtered.is_empty());
    }

    #[test]
    fn shift_j_k_scrolls_preview() {
        let mut picker = test_picker();
        assert_eq!(picker.preview_scroll, 0);
        picker.handle_key(shift_key(KeyCode::Char('J')));
        assert_eq!(picker.preview_scroll, 1);
        picker.handle_key(shift_key(KeyCode::Char('K')));
        assert_eq!(picker.preview_scroll, 0);
        // Should not go below 0
        picker.handle_key(shift_key(KeyCode::Char('K')));
        assert_eq!(picker.preview_scroll, 0);
    }

    #[test]
    fn shift_h_l_scrolls_preview_horizontally() {
        let mut picker = test_picker();
        assert_eq!(picker.preview_horizontal_scroll, 0);
        picker.handle_key(shift_key(KeyCode::Char('L')));
        assert_eq!(picker.preview_horizontal_scroll, HORIZONTAL_SCROLL_COLS);
        picker.handle_key(shift_key(KeyCode::Char('H')));
        assert_eq!(picker.preview_horizontal_scroll, 0);
    }

    #[test]
    fn mouse_scroll_clamps_preview_to_visible_content() {
        let mut picker = test_picker();
        let cols = 100;
        let rows = 20;
        let content_h = PrListPicker::preview_content_height_for_surface(cols, rows).unwrap();
        let preview_len = picker.preview_lines().len();
        let max_scroll = picker.preview_max_scroll(preview_len, content_h);

        for _ in 0..200 {
            let result = picker.handle_mouse_scroll(MouseEventKind::ScrollDown, cols, rows);
            assert!(matches!(result, EventResult::Consumed));
        }

        assert_eq!(picker.preview_scroll, max_scroll);
    }

    #[test]
    fn mouse_scroll_ignored_without_split_preview_panel() {
        let mut picker = test_picker();
        let result = picker.handle_mouse_scroll(MouseEventKind::ScrollDown, 40, 20);
        assert!(matches!(result, EventResult::Ignored));
    }

    #[test]
    fn navigate_to_second_pr_and_open() {
        let mut picker = test_picker();
        picker.handle_key(key(KeyCode::Char('j')));
        let result = picker.handle_key(key(KeyCode::Char('o')));
        match result {
            EventResult::Action(Action::App(AppAction::Integration(
                IntegrationAction::OpenPrUrl(url),
            ))) => {
                assert_eq!(url, "https://github.com/user/repo/pull/41");
            }
            _ => panic!("Expected OpenPrUrl for PR #41"),
        }
    }

    #[test]
    fn find_by_number() {
        let mut picker = test_picker();
        picker.handle_key(key(KeyCode::Char('/')));
        picker.handle_key(key(KeyCode::Char('#')));
        picker.handle_key(key(KeyCode::Char('4')));
        picker.handle_key(key(KeyCode::Char('0')));
        // Should match #40 "Update dependencies"
        assert!(!picker.filtered.is_empty());
        let top_entry = &picker.entries[picker.filtered[0]];
        assert_eq!(top_entry.number, 40);
    }

    #[test]
    fn find_mode_navigation() {
        let mut picker = test_picker();
        picker.handle_key(key(KeyCode::Char('/')));
        picker.handle_key(ctrl_key('n'));
        assert_eq!(picker.selected, 1);
        picker.handle_key(ctrl_key('p'));
        assert_eq!(picker.selected, 0);
        picker.handle_key(key(KeyCode::Down));
        assert_eq!(picker.selected, 1);
        picker.handle_key(key(KeyCode::Up));
        assert_eq!(picker.selected, 0);
    }
}
