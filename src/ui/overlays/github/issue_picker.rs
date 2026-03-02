use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEventKind};
use crossterm::style::Color;
use serde::Deserialize;
use serde_json::Value;

use crate::input::action::{Action, AppAction, IntegrationAction, UiAction};
use crate::ui::framework::cell::CellStyle;
use crate::ui::framework::component::EventResult;
use crate::ui::framework::surface::Surface;
use crate::ui::shared::filtering::fuzzy_match;
use crate::core_lib::text::input::delete_prev_word_input;
use crate::core_lib::ui::text::{display_width, slice_display_window, truncate_to_width};

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
struct GhIssueEntry {
    number: u64,
    title: String,
    body: String,
    url: String,
    state: String,
    author: GhAuthor,
    created_at: String,
    #[serde(default)]
    labels: Vec<GhLabel>,
    #[serde(default)]
    comments: Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssueCommentEntry {
    pub author: String,
    pub body: String,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct IssueEntry {
    pub number: u64,
    pub title: String,
    pub body: String,
    pub url: String,
    pub state: String,
    pub author: String,
    pub created_at: String,
    pub labels: Vec<String>,
    pub comments: Vec<IssueCommentEntry>,
    pub comment_count: usize,
}

fn comments_from_array(items: &[Value]) -> Vec<IssueCommentEntry> {
    items
        .iter()
        .filter_map(|item| {
            let body = item
                .get("body")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let author = item
                .get("author")
                .and_then(|a| a.get("login"))
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_string();
            let created_at = item
                .get("createdAt")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();

            if body.is_empty() && created_at.is_empty() && author == "unknown" {
                None
            } else {
                Some(IssueCommentEntry {
                    author,
                    body,
                    created_at,
                })
            }
        })
        .collect()
}

fn parse_issue_comments(value: &Value) -> (Vec<IssueCommentEntry>, usize) {
    match value {
        Value::Array(items) => {
            let comments = comments_from_array(items);
            (comments, items.len())
        }
        Value::Object(map) => {
            let total_count = map
                .get("totalCount")
                .and_then(Value::as_u64)
                .and_then(|n| usize::try_from(n).ok())
                .unwrap_or(0);
            let comments = map
                .get("nodes")
                .and_then(Value::as_array)
                .map(|nodes| comments_from_array(nodes))
                .or_else(|| {
                    map.get("items")
                        .and_then(Value::as_array)
                        .map(|it| comments_from_array(it))
                })
                .unwrap_or_default();
            let count = if total_count > 0 {
                total_count
            } else {
                comments.len()
            };
            (comments, count)
        }
        Value::Number(n) => {
            let count = n
                .as_u64()
                .and_then(|v| usize::try_from(v).ok())
                .unwrap_or(0);
            (Vec::new(), count)
        }
        _ => (Vec::new(), 0),
    }
}

pub fn parse_gh_issue_json(json: &str) -> Result<Vec<IssueEntry>, String> {
    let gh_entries: Vec<GhIssueEntry> =
        serde_json::from_str(json).map_err(|e| format!("Failed to parse gh output: {}", e))?;

    Ok(gh_entries
        .into_iter()
        .map(|e| {
            let (comments, comment_count) = parse_issue_comments(&e.comments);
            IssueEntry {
                number: e.number,
                title: e.title,
                body: e.body,
                url: e.url,
                state: e.state,
                author: e.author.login,
                created_at: e.created_at,
                labels: e.labels.into_iter().map(|l| l.name).collect(),
                comments,
                comment_count,
            }
        })
        .collect())
}

pub struct IssueListPicker {
    entries: Vec<IssueEntry>,
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

impl IssueListPicker {
    pub fn new(entries: Vec<IssueEntry>) -> Self {
        let msg = if entries.is_empty() {
            Some("No issues found".to_string())
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

    fn selected_entry(&self) -> Option<&IssueEntry> {
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
                "No issue selected".to_string(),
                CellStyle {
                    dim: true,
                    ..CellStyle::default()
                },
            )];
        };

        let mut lines: Vec<(String, CellStyle)> = Vec::new();
        let state_style = match entry.state.as_str() {
            "OPEN" | "open" => CellStyle {
                fg: Some(Color::Green),
                bold: true,
                ..CellStyle::default()
            },
            "CLOSED" | "closed" => CellStyle {
                fg: Some(Color::Red),
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
        lines.push((
            format!("Title: {}", entry.title),
            CellStyle {
                bold: true,
                ..CellStyle::default()
            },
        ));
        lines.push((String::new(), CellStyle::default()));
        lines.push((
            "Description:".to_string(),
            CellStyle {
                fg: Some(Color::Cyan),
                ..CellStyle::default()
            },
        ));

        let body = if entry.body.is_empty() {
            "(no description)"
        } else {
            &entry.body
        };
        for line in body.lines() {
            lines.push((line.to_string(), CellStyle::default()));
        }

        lines.push((String::new(), CellStyle::default()));
        lines.push((
            format!("Comments ({}):", entry.comment_count),
            CellStyle {
                fg: Some(Color::Cyan),
                bold: true,
                ..CellStyle::default()
            },
        ));

        if entry.comment_count == 0 {
            lines.push((
                "(no comments)".to_string(),
                CellStyle {
                    dim: true,
                    ..CellStyle::default()
                },
            ));
            return lines;
        }

        if entry.comments.is_empty() {
            lines.push((
                "(comments unavailable in this listing)".to_string(),
                CellStyle {
                    dim: true,
                    ..CellStyle::default()
                },
            ));
            return lines;
        }

        for (idx, comment) in entry.comments.iter().enumerate() {
            if idx > 0 {
                lines.push((String::new(), CellStyle::default()));
            }
            let created_display = comment
                .created_at
                .split('T')
                .next()
                .unwrap_or(&comment.created_at);
            lines.push((
                format!("- @{} ({})", comment.author, created_display),
                CellStyle {
                    fg: Some(Color::Yellow),
                    ..CellStyle::default()
                },
            ));
            if comment.body.trim().is_empty() {
                lines.push((
                    "(empty comment)".to_string(),
                    CellStyle {
                        dim: true,
                        ..CellStyle::default()
                    },
                ));
            } else {
                for line in comment.body.lines() {
                    lines.push((line.to_string(), CellStyle::default()));
                }
            }
        }

        lines
    }

    fn delete_prev_word(&mut self) {
        delete_prev_word_input(&mut self.find_input);
    }

    fn handle_find_key(&mut self, key: KeyEvent) -> EventResult {
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            return match key.code {
                KeyCode::Char('q') => {
                    EventResult::Action(Action::Ui(UiAction::CloseIssueListPicker))
                }
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
                    description: "Issue URL".to_string(),
                },
            ))),
            KeyCode::Char('n') => EventResult::Action(Action::App(AppAction::Integration(
                IntegrationAction::CopyToClipboard {
                    text: format!("#{}", entry.number),
                    description: "Issue number".to_string(),
                },
            ))),
            KeyCode::Char('t') => EventResult::Action(Action::App(AppAction::Integration(
                IntegrationAction::CopyToClipboard {
                    text: entry.title.clone(),
                    description: "Issue title".to_string(),
                },
            ))),
            _ => EventResult::Consumed,
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> EventResult {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('q') {
            return EventResult::Action(Action::Ui(UiAction::CloseIssueListPicker));
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
                    EventResult::Action(Action::App(AppAction::Integration(
                        IntegrationAction::OpenIssueUrl(entry.url.clone()),
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
                EventResult::Action(Action::Ui(UiAction::CloseIssueListPicker))
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

            let cursor = self.render_issue_panel(surface, offset_x, offset_y, left_w, popup_h);
            self.render_preview_panel(surface, right_x, offset_y, right_w, popup_h);
            cursor
        } else {
            self.render_issue_panel(surface, offset_x, offset_y, popup_w, popup_h)
        }
    }

    fn render_issue_panel(
        &mut self,
        surface: &mut Surface,
        x: usize,
        y: usize,
        w: usize,
        h: usize,
    ) -> Option<(u16, u16)> {
        let inner_w = w.saturating_sub(2);
        let default_style = CellStyle::default();

        let content_start = 2;
        let content_end = h.saturating_sub(2);
        let content_h = content_end.saturating_sub(content_start);

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
                surface.put_str(x, y + row, "\u{2502}", &default_style);
                let title = format!(" Issues ({})", self.filtered.len());
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
            Some(((x + 1 + used) as u16, find_row as u16))
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

    fn test_entries() -> Vec<IssueEntry> {
        vec![
            IssueEntry {
                number: 15,
                title: "Add docs for API".to_string(),
                body: "Please add details for API behavior.".to_string(),
                url: "https://github.com/user/repo/issues/15".to_string(),
                state: "OPEN".to_string(),
                author: "alice".to_string(),
                created_at: "2026-01-01T10:30:00Z".to_string(),
                labels: vec!["docs".to_string()],
                comments: vec![
                    IssueCommentEntry {
                        author: "bob".to_string(),
                        body: "I can work on this.".to_string(),
                        created_at: "2026-01-02T09:00:00Z".to_string(),
                    },
                    IssueCommentEntry {
                        author: "charlie".to_string(),
                        body: "Thanks, assigned.".to_string(),
                        created_at: "2026-01-03T09:00:00Z".to_string(),
                    },
                ],
                comment_count: 2,
            },
            IssueEntry {
                number: 12,
                title: "Crash on launch".to_string(),
                body: "Steps to repro".to_string(),
                url: "https://github.com/user/repo/issues/12".to_string(),
                state: "CLOSED".to_string(),
                author: "dora".to_string(),
                created_at: "2026-01-01T08:00:00Z".to_string(),
                labels: vec![],
                comments: vec![],
                comment_count: 0,
            },
        ]
    }

    fn test_picker() -> IssueListPicker {
        IssueListPicker::new(test_entries())
    }

    #[test]
    fn parse_valid_json_with_comment_array() {
        let json = r#"[
            {
                "number": 1,
                "title": "Test issue",
                "body": "body text",
                "url": "https://github.com/x/y/issues/1",
                "state": "OPEN",
                "author": {"login": "user1"},
                "createdAt": "2026-01-01T00:00:00Z",
                "labels": [{"name": "bug"}],
                "comments": [
                    {
                        "author": {"login": "user2"},
                        "body": "first comment",
                        "createdAt": "2026-01-02T00:00:00Z"
                    }
                ]
            }
        ]"#;
        let entries = parse_gh_issue_json(json).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].number, 1);
        assert_eq!(entries[0].title, "Test issue");
        assert_eq!(entries[0].author, "user1");
        assert_eq!(entries[0].labels, vec!["bug".to_string()]);
        assert_eq!(entries[0].comment_count, 1);
        assert_eq!(entries[0].comments[0].author, "user2");
    }

    #[test]
    fn parse_comments_connection_shape() {
        let json = r#"[
            {
                "number": 1,
                "title": "Issue",
                "body": "",
                "url": "https://github.com/x/y/issues/1",
                "state": "OPEN",
                "author": {"login": "user1"},
                "createdAt": "2026-01-01T00:00:00Z",
                "labels": [],
                "comments": {
                    "totalCount": 3,
                    "nodes": [
                        {
                            "author": {"login": "user2"},
                            "body": "hello",
                            "createdAt": "2026-01-02T00:00:00Z"
                        }
                    ]
                }
            }
        ]"#;
        let entries = parse_gh_issue_json(json).unwrap();
        assert_eq!(entries[0].comment_count, 3);
        assert_eq!(entries[0].comments.len(), 1);
    }

    #[test]
    fn parse_invalid_json() {
        assert!(parse_gh_issue_json("not json").is_err());
    }

    #[test]
    fn open_produces_open_issue_url() {
        let mut picker = test_picker();
        let result = picker.handle_key(key(KeyCode::Char('o')));
        match result {
            EventResult::Action(Action::App(AppAction::Integration(
                IntegrationAction::OpenIssueUrl(url),
            ))) => {
                assert_eq!(url, "https://github.com/user/repo/issues/15");
            }
            _ => panic!("Expected OpenIssueUrl action"),
        }
    }

    #[test]
    fn find_mode_filters_entries() {
        let mut picker = test_picker();
        picker.handle_key(key(KeyCode::Char('/')));
        picker.handle_key(key(KeyCode::Char('c')));
        picker.handle_key(key(KeyCode::Char('r')));
        picker.handle_key(key(KeyCode::Char('a')));
        picker.handle_key(key(KeyCode::Char('s')));
        picker.handle_key(key(KeyCode::Char('h')));
        assert_eq!(picker.filtered.len(), 1);
        assert_eq!(picker.entries[picker.filtered[0]].number, 12);
    }

    #[test]
    fn copy_menu_url() {
        let mut picker = test_picker();
        picker.handle_key(key(KeyCode::Char('c')));
        let result = picker.handle_key(key(KeyCode::Char('u')));
        match result {
            EventResult::Action(Action::App(AppAction::Integration(
                IntegrationAction::CopyToClipboard { text, description },
            ))) => {
                assert_eq!(text, "https://github.com/user/repo/issues/15");
                assert_eq!(description, "Issue URL");
            }
            _ => panic!("Expected CopyToClipboard action"),
        }
    }

    #[test]
    fn esc_closes() {
        let mut picker = test_picker();
        let result = picker.handle_key(key(KeyCode::Esc));
        assert!(matches!(
            result,
            EventResult::Action(Action::Ui(UiAction::CloseIssueListPicker))
        ));
    }

    #[test]
    fn ctrl_q_closes() {
        let mut picker = test_picker();
        let result = picker.handle_key(ctrl_key('q'));
        assert!(matches!(
            result,
            EventResult::Action(Action::Ui(UiAction::CloseIssueListPicker))
        ));
    }

    #[test]
    fn shift_j_k_scrolls_preview() {
        let mut picker = test_picker();
        assert_eq!(picker.preview_scroll, 0);
        picker.handle_key(shift_key(KeyCode::Char('J')));
        assert_eq!(picker.preview_scroll, 1);
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
        let content_h = IssueListPicker::preview_content_height_for_surface(cols, rows).unwrap();
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
    fn preview_includes_title_description_and_comments() {
        let picker = test_picker();
        let text = picker
            .preview_lines()
            .into_iter()
            .map(|(line, _)| line)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("Title: Add docs for API"));
        assert!(text.contains("Description:"));
        assert!(text.contains("Comments (2):"));
        assert!(text.contains("I can work on this."));
    }
}
