use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEventKind};
use crossterm::style::Color;
use std::path::PathBuf;
use std::sync::mpsc;

use crate::command::commit_log_runtime::{
    CommitDetail, CommitEntry, CommitLogCommand, CommitLogEvent,
};
use crate::command::git;
use crate::input::action::{Action, AppAction, IntegrationAction, UiAction, WorkspaceAction};
use crate::ui::framework::cell::CellStyle;
use crate::ui::framework::component::EventResult;
use crate::ui::framework::surface::Surface;
use crate::ui::text::{slice_display_window, truncate_to_width};
use crate::ui::text_input::delete_prev_word_input;

const PAGE_SIZE: usize = 100;
const DIFF_SPLIT_THRESHOLD: usize = 60;
const MOUSE_SCROLL_LINES: usize = 3;
const HORIZONTAL_SCROLL_COLS: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ViewMode {
    List,
    Detail,
}

pub struct CommitLogView {
    project_root: PathBuf,
    branch: String,
    commits: Vec<CommitEntry>,
    selected: usize,
    scroll_offset: usize,
    view_mode: ViewMode,
    // Detail view state
    detail: Option<CommitDetail>,
    detail_scroll: usize,
    detail_horizontal_scroll: usize,
    // Diff panel (right side in list mode)
    diff_lines: Vec<String>,
    diff_scroll: usize,
    diff_horizontal_scroll: usize,
    diff_loading: bool,
    // Find/filter
    find_active: bool,
    find_input: String,
    // Async
    runtime_tx: Option<mpsc::Sender<CommitLogCommand>>,
    loading: bool,
    has_more: bool,
    message: Option<String>,
}

impl CommitLogView {
    pub fn new(
        project_root: PathBuf,
        runtime_tx: Option<mpsc::Sender<CommitLogCommand>>,
    ) -> Self {
        let branch = git::git_branch_in(&project_root).unwrap_or_else(|_| "???".to_string());
        let mut view = Self {
            project_root: project_root.clone(),
            branch,
            commits: Vec::new(),
            selected: 0,
            scroll_offset: 0,
            view_mode: ViewMode::List,
            detail: None,
            detail_scroll: 0,
            detail_horizontal_scroll: 0,
            diff_lines: Vec::new(),
            diff_scroll: 0,
            diff_horizontal_scroll: 0,
            diff_loading: false,
            find_active: false,
            find_input: String::new(),
            runtime_tx,
            loading: true,
            has_more: false,
            message: Some("Loading commits...".to_string()),
        };
        view.request_page(0);
        view
    }

    fn request_page(&mut self, skip: usize) {
        if let Some(tx) = &self.runtime_tx {
            self.loading = true;
            let _ = tx.send(CommitLogCommand::LoadPage {
                project_root: self.project_root.clone(),
                skip,
                count: PAGE_SIZE,
            });
        }
    }

    fn request_detail(&mut self, hash: &str) {
        if let Some(tx) = &self.runtime_tx {
            self.diff_loading = true;
            let _ = tx.send(CommitLogCommand::LoadDetail {
                project_root: self.project_root.clone(),
                hash: hash.to_string(),
            });
        }
    }

    pub fn apply_event(&mut self, event: CommitLogEvent) {
        match event {
            CommitLogEvent::PageLoaded {
                commits,
                has_more,
                is_append,
            } => {
                self.loading = false;
                self.has_more = has_more;
                if is_append {
                    self.commits.extend(commits);
                } else {
                    self.commits = commits;
                    self.selected = 0;
                    self.scroll_offset = 0;
                }
                if self.message.as_deref() == Some("Loading commits...") {
                    self.message = None;
                }
                self.request_selected_diff();
            }
            CommitLogEvent::DetailLoaded { hash, detail } => {
                self.diff_loading = false;
                if self.view_mode == ViewMode::Detail {
                    // Check the detail is for the currently viewed commit
                    if self
                        .commits
                        .get(self.selected)
                        .is_some_and(|c| c.full_hash == hash)
                    {
                        self.detail = Some(detail);
                    }
                } else {
                    // In list mode, we use diff_lines from the detail
                    if self
                        .commits
                        .get(self.selected)
                        .is_some_and(|c| c.full_hash == hash)
                    {
                        self.diff_lines = detail.diff_lines;
                        self.diff_scroll = 0;
                        self.diff_horizontal_scroll = 0;
                    }
                }
            }
            CommitLogEvent::Error { message } => {
                self.loading = false;
                self.diff_loading = false;
                self.message = Some(message);
            }
        }
    }

    fn request_selected_diff(&mut self) {
        if let Some(commit) = self.commits.get(self.selected) {
            let hash = commit.full_hash.clone();
            self.diff_lines.clear();
            self.diff_scroll = 0;
            self.diff_horizontal_scroll = 0;
            self.request_detail(&hash);
        }
    }

    // Navigation

    fn move_down(&mut self) {
        if self.selected + 1 < self.commits.len() {
            self.selected += 1;
            self.request_selected_diff();
            // Auto-load more when near the bottom
            if self.has_more && !self.loading && self.selected + 20 >= self.commits.len() {
                let skip = self.commits.len();
                self.request_page(skip);
            }
        }
    }

    fn move_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
            self.request_selected_diff();
        }
    }

    fn move_down_lines(&mut self, n: usize) {
        let max = self.commits.len().saturating_sub(1);
        let new_sel = (self.selected + n).min(max);
        if new_sel != self.selected {
            self.selected = new_sel;
            self.request_selected_diff();
            if self.has_more && !self.loading && self.selected + 20 >= self.commits.len() {
                let skip = self.commits.len();
                self.request_page(skip);
            }
        }
    }

    fn move_up_lines(&mut self, n: usize) {
        let new_sel = self.selected.saturating_sub(n);
        if new_sel != self.selected {
            self.selected = new_sel;
            self.request_selected_diff();
        }
    }

    // Diff scrolling

    fn scroll_diff_down(&mut self) {
        self.diff_scroll = self.diff_scroll.saturating_add(1);
    }

    fn scroll_diff_up(&mut self) {
        self.diff_scroll = self.diff_scroll.saturating_sub(1);
    }

    fn scroll_diff_right(&mut self) {
        self.diff_horizontal_scroll = self
            .diff_horizontal_scroll
            .saturating_add(HORIZONTAL_SCROLL_COLS);
    }

    fn scroll_diff_left(&mut self) {
        self.diff_horizontal_scroll = self
            .diff_horizontal_scroll
            .saturating_sub(HORIZONTAL_SCROLL_COLS);
    }

    fn clamp_diff_scroll(&mut self, content_h: usize) {
        let max = self.diff_lines.len().saturating_sub(content_h);
        if self.diff_scroll > max {
            self.diff_scroll = max;
        }
    }

    // Detail view scrolling

    fn detail_scroll_down(&mut self) {
        self.detail_scroll = self.detail_scroll.saturating_add(1);
    }

    fn detail_scroll_up(&mut self) {
        self.detail_scroll = self.detail_scroll.saturating_sub(1);
    }

    fn detail_scroll_right(&mut self) {
        self.detail_horizontal_scroll = self
            .detail_horizontal_scroll
            .saturating_add(HORIZONTAL_SCROLL_COLS);
    }

    fn detail_scroll_left(&mut self) {
        self.detail_horizontal_scroll = self
            .detail_horizontal_scroll
            .saturating_sub(HORIZONTAL_SCROLL_COLS);
    }

    fn exit_detail(&mut self) {
        self.view_mode = ViewMode::List;
        self.detail = None;
        self.request_selected_diff();
    }

    fn copy_selected_hash(&self) -> Option<EventResult> {
        self.commits.get(self.selected).map(|c| {
            EventResult::Action(Action::App(AppAction::Integration(
                IntegrationAction::CopyToClipboard {
                    text: c.full_hash.clone(),
                    description: "commit hash".to_string(),
                },
            )))
        })
    }

    fn popup_size(cols: usize, rows: usize) -> (usize, usize) {
        ((cols * 80 / 100).max(3), (rows * 80 / 100).max(3))
    }

    // Mouse handling

    pub fn handle_mouse_scroll(
        &mut self,
        kind: MouseEventKind,
        col: u16,
        _row: u16,
        cols: usize,
        rows: usize,
    ) {
        let delta = MOUSE_SCROLL_LINES;
        let (popup_w, popup_h) = Self::popup_size(cols, rows);
        let offset_x = (cols.saturating_sub(popup_w)) / 2;

        let in_right_panel = if popup_w >= DIFF_SPLIT_THRESHOLD {
            let gap = 2;
            let left_w = (popup_w - gap) / 2;
            (col as usize) >= offset_x + left_w + gap
        } else {
            false
        };

        match kind {
            MouseEventKind::ScrollDown => {
                if self.view_mode == ViewMode::Detail {
                    self.detail_scroll = self.detail_scroll.saturating_add(delta);
                } else if in_right_panel {
                    self.diff_scroll = self.diff_scroll.saturating_add(delta);
                    self.clamp_diff_scroll(popup_h.saturating_sub(2));
                } else {
                    self.move_down_lines(delta);
                }
            }
            MouseEventKind::ScrollUp => {
                if self.view_mode == ViewMode::Detail {
                    self.detail_scroll = self.detail_scroll.saturating_sub(delta);
                } else if in_right_panel {
                    self.diff_scroll = self.diff_scroll.saturating_sub(delta);
                } else {
                    self.move_up_lines(delta);
                }
            }
            _ => {}
        }
    }

    // Key handling

    fn handle_find_key(&mut self, key: KeyEvent) -> EventResult {
        match key.code {
            KeyCode::Esc => {
                self.find_active = false;
                self.find_input.clear();
                EventResult::Consumed
            }
            KeyCode::Enter => {
                self.find_active = false;
                EventResult::Consumed
            }
            KeyCode::Backspace => {
                self.find_input.pop();
                EventResult::Consumed
            }
            KeyCode::Char('w')
                if key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                delete_prev_word_input(&mut self.find_input);
                EventResult::Consumed
            }
            KeyCode::Char(c) => {
                if !key.modifiers.contains(KeyModifiers::CONTROL) {
                    self.find_input.push(c);
                }
                EventResult::Consumed
            }
            _ => EventResult::Consumed,
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> EventResult {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('q') {
            return EventResult::Action(Action::Ui(UiAction::CloseCommitLog));
        }

        if self.find_active {
            return self.handle_find_key(key);
        }

        if self.view_mode == ViewMode::Detail {
            return self.handle_detail_key(key);
        }

        let has_shift = key.modifiers.contains(KeyModifiers::SHIFT);

        if key.modifiers.contains(KeyModifiers::CONTROL) {
            return match key.code {
                KeyCode::Char('n') => {
                    if has_shift {
                        self.scroll_diff_down();
                    } else {
                        self.move_down();
                    }
                    EventResult::Consumed
                }
                KeyCode::Char('p') => {
                    if has_shift {
                        self.scroll_diff_up();
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
                    self.scroll_diff_down();
                    EventResult::Consumed
                }
                KeyCode::Char('K') | KeyCode::Up => {
                    self.scroll_diff_up();
                    EventResult::Consumed
                }
                KeyCode::Char('L') | KeyCode::Right => {
                    self.scroll_diff_right();
                    EventResult::Consumed
                }
                KeyCode::Char('H') | KeyCode::Left => {
                    self.scroll_diff_left();
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
            KeyCode::Char('y') => self.copy_selected_hash().unwrap_or(EventResult::Consumed),
            KeyCode::Char('o') | KeyCode::Enter => {
                if let Some(commit) = self.commits.get(self.selected) {
                    let hash = commit.full_hash.clone();
                    EventResult::Action(Action::App(AppAction::Workspace(
                        WorkspaceAction::OpenCommitDiffView(hash),
                    )))
                } else {
                    EventResult::Consumed
                }
            }
            KeyCode::Char('r') => {
                self.commits.clear();
                self.selected = 0;
                self.scroll_offset = 0;
                self.diff_lines.clear();
                self.message = Some("Loading commits...".to_string());
                self.request_page(0);
                EventResult::Consumed
            }
            KeyCode::Esc | KeyCode::Char('q') => {
                EventResult::Action(Action::Ui(UiAction::CloseCommitLog))
            }
            _ => EventResult::Consumed,
        }
    }

    fn handle_detail_key(&mut self, key: KeyEvent) -> EventResult {
        let has_shift = key.modifiers.contains(KeyModifiers::SHIFT);

        if has_shift {
            return match key.code {
                KeyCode::Char('L') | KeyCode::Right => {
                    self.detail_scroll_right();
                    EventResult::Consumed
                }
                KeyCode::Char('H') | KeyCode::Left => {
                    self.detail_scroll_left();
                    EventResult::Consumed
                }
                _ => EventResult::Consumed,
            };
        }

        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                self.detail_scroll_down();
                EventResult::Consumed
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.detail_scroll_up();
                EventResult::Consumed
            }
            KeyCode::Char('y') => self.copy_selected_hash().unwrap_or(EventResult::Consumed),
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Backspace => {
                self.exit_detail();
                EventResult::Consumed
            }
            _ => EventResult::Consumed,
        }
    }

    // Rendering

    pub fn render_overlay(&mut self, surface: &mut Surface) -> Option<(u16, u16)> {
        let cols = surface.width;
        let rows = surface.height;
        let (popup_w, popup_h) = Self::popup_size(cols, rows);
        let offset_x = (cols.saturating_sub(popup_w)) / 2;
        let offset_y = (rows.saturating_sub(popup_h)) / 2;

        if self.view_mode == ViewMode::Detail {
            return self.render_detail_view(surface, offset_x, offset_y, popup_w, popup_h);
        }

        if popup_w >= DIFF_SPLIT_THRESHOLD {
            let gap = 2;
            let left_w = (popup_w - gap) / 2;
            let right_w = popup_w - gap - left_w;
            let right_x = offset_x + left_w + gap;

            let cursor = self.render_commit_panel(surface, offset_x, offset_y, left_w, popup_h);
            self.render_diff_panel(surface, right_x, offset_y, right_w, popup_h);
            cursor
        } else {
            self.render_commit_panel(surface, offset_x, offset_y, popup_w, popup_h)
        }
    }

    fn render_commit_panel(
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
        // row 1: branch/title row
        // row 2..h-2: commit list
        // row h-2: message/hint row
        // row h-1: bottom border

        let content_start = 2;
        let content_end = h.saturating_sub(2);
        let content_h = content_end.saturating_sub(content_start);

        // Filter commits if find is active
        let visible_indices: Vec<usize> = if !self.find_input.is_empty() {
            let query = self.find_input.to_lowercase();
            self.commits
                .iter()
                .enumerate()
                .filter(|(_, c)| {
                    c.message.to_lowercase().contains(&query)
                        || c.hash.contains(&query)
                        || c.author.to_lowercase().contains(&query)
                })
                .map(|(i, _)| i)
                .collect()
        } else {
            (0..self.commits.len()).collect()
        };

        // Find selected position in visible list
        let sel_vis_pos = visible_indices
            .iter()
            .position(|&i| i == self.selected)
            .unwrap_or(0);

        // Adjust scroll
        if sel_vis_pos < self.scroll_offset {
            self.scroll_offset = sel_vis_pos;
        }
        if sel_vis_pos >= self.scroll_offset + content_h {
            self.scroll_offset = sel_vis_pos.saturating_sub(content_h.saturating_sub(1));
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
                let title = format!(
                    " Commit Log \u{e0a0} {} ({})",
                    self.branch,
                    if self.loading {
                        "loading...".to_string()
                    } else {
                        format!("{} commits", self.commits.len())
                    }
                );
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
                // Message/hint row
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
                } else if let Some(ref msg) = self.message {
                    let (truncated, used) = truncate_to_width(msg, inner_w);
                    surface.put_str(x + 1, y + row, truncated, &row_style);
                    if used < inner_w {
                        surface.fill_region(x + 1 + used, y + row, inner_w - used, ' ', &row_style);
                    }
                } else {
                    let hint = "o:detail y:copy-hash /:find r:refresh q:close";
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
            } else {
                // Commit rows
                let content_row = row - content_start;
                let vis_idx = self.scroll_offset + content_row;

                surface.put_str(x, y + row, "\u{2502}", &default_style);

                if vis_idx < visible_indices.len() {
                    let commit_idx = visible_indices[vis_idx];
                    let commit = &self.commits[commit_idx];
                    let is_selected = commit_idx == self.selected;

                    // Format: hash message  date
                    let date_w = commit.date.len().min(inner_w / 4);
                    let rest_w = inner_w.saturating_sub(date_w + 1);
                    let style = if is_selected {
                        CellStyle {
                            reverse: true,
                            ..CellStyle::default()
                        }
                    } else {
                        CellStyle::default()
                    };

                    let hash_style = if is_selected {
                        CellStyle {
                            reverse: true,
                            fg: Some(Color::Yellow),
                            ..CellStyle::default()
                        }
                    } else {
                        CellStyle {
                            fg: Some(Color::Yellow),
                            ..CellStyle::default()
                        }
                    };

                    let date_style = if is_selected {
                        CellStyle {
                            reverse: true,
                            dim: true,
                            ..CellStyle::default()
                        }
                    } else {
                        CellStyle {
                            dim: true,
                            ..CellStyle::default()
                        }
                    };

                    // Draw hash
                    let hash_len = commit.hash.len().min(rest_w);
                    surface.put_str(x + 1, y + row, &commit.hash[..hash_len], &hash_style);

                    // Draw message
                    let msg_start = hash_len + 1;
                    if msg_start < rest_w {
                        let msg_w = rest_w - msg_start;
                        let (truncated, used) = truncate_to_width(&commit.message, msg_w);
                        surface.put_str(x + 1 + msg_start, y + row, truncated, &style);
                        if used < msg_w {
                            surface.fill_region(
                                x + 1 + msg_start + used,
                                y + row,
                                msg_w - used,
                                ' ',
                                &style,
                            );
                        }
                    }

                    // Draw date right-aligned
                    let date_x = x + 1 + inner_w - date_w;
                    let (truncated, _) = truncate_to_width(&commit.date, date_w);
                    surface.put_str(date_x, y + row, truncated, &date_style);
                } else {
                    surface.fill_region(x + 1, y + row, inner_w, ' ', &default_style);
                }

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

    fn render_diff_panel(
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
        self.clamp_diff_scroll(content_h);

        let is_loading = self.diff_loading && self.diff_lines.is_empty();
        let placeholder = if is_loading {
            Some((
                "Loading diff...",
                CellStyle {
                    dim: true,
                    ..CellStyle::default()
                },
            ))
        } else if self.commits.is_empty() {
            Some((
                "No commits",
                CellStyle {
                    dim: true,
                    ..CellStyle::default()
                },
            ))
        } else if self.diff_lines.is_empty() && !self.diff_loading {
            Some((
                "No diff",
                CellStyle {
                    dim: true,
                    ..CellStyle::default()
                },
            ))
        } else {
            None
        };

        for row in 0..h {
            if row == 0 {
                surface.put_str(x, y + row, "\u{250c}", &default_style);
                surface.fill_region(x + 1, y + row, inner_w, '\u{2500}', &default_style);
                surface.put_str(x + 1 + inner_w, y + row, "\u{2510}", &default_style);
                continue;
            }
            if row == h - 1 {
                surface.put_str(x, y + row, "\u{2514}", &default_style);
                surface.fill_region(x + 1, y + row, inner_w, '\u{2500}', &default_style);
                surface.put_str(x + 1 + inner_w, y + row, "\u{2518}", &default_style);
                continue;
            }

            surface.put_str(x, y + row, "\u{2502}", &default_style);

            if let Some((text, style)) = placeholder {
                let placeholder_row = 1 + content_h / 2;
                if row == placeholder_row {
                    let (truncated, used) = truncate_to_width(text, inner_w);
                    surface.put_str(x + 1, y + row, truncated, &style);
                    if used < inner_w {
                        surface.fill_region(
                            x + 1 + used,
                            y + row,
                            inner_w - used,
                            ' ',
                            &default_style,
                        );
                    }
                } else {
                    surface.fill_region(x + 1, y + row, inner_w, ' ', &default_style);
                }
            } else {
                let line_idx = self.diff_scroll + (row - 1);
                if line_idx < self.diff_lines.len() && (row - 1) < content_h {
                    let line = &self.diff_lines[line_idx];
                    let style = diff_line_style(line);
                    let window =
                        slice_display_window(line, self.diff_horizontal_scroll, inner_w);
                    surface.put_str(x + 1, y + row, window.visible, &style);
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
            }

            surface.put_str(x + 1 + inner_w, y + row, "\u{2502}", &default_style);
        }
    }

    fn render_detail_view(
        &mut self,
        surface: &mut Surface,
        x: usize,
        y: usize,
        w: usize,
        h: usize,
    ) -> Option<(u16, u16)> {
        let inner_w = w.saturating_sub(2);
        let content_h = h.saturating_sub(2);
        let default_style = CellStyle::default();

        // Build detail lines
        let detail_lines = self.build_detail_lines();

        // Clamp scroll
        let max_scroll = detail_lines.len().saturating_sub(content_h);
        if self.detail_scroll > max_scroll {
            self.detail_scroll = max_scroll;
        }

        for row in 0..h {
            if row == 0 {
                surface.put_str(x, y + row, "\u{250c}", &default_style);
                surface.fill_region(x + 1, y + row, inner_w, '\u{2500}', &default_style);
                surface.put_str(x + 1 + inner_w, y + row, "\u{2510}", &default_style);
                continue;
            }
            if row == h - 1 {
                // Bottom border with hint
                surface.put_str(x, y + row, "\u{2514}", &default_style);
                surface.fill_region(x + 1, y + row, inner_w, '\u{2500}', &default_style);
                surface.put_str(x + 1 + inner_w, y + row, "\u{2518}", &default_style);
                continue;
            }

            surface.put_str(x, y + row, "\u{2502}", &default_style);

            let line_idx = self.detail_scroll + (row - 1);
            if line_idx < detail_lines.len() && (row - 1) < content_h {
                let (line, style) = &detail_lines[line_idx];
                let window =
                    slice_display_window(line, self.detail_horizontal_scroll, inner_w);
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

        None
    }

    fn build_detail_lines(&self) -> Vec<(String, CellStyle)> {
        let mut lines: Vec<(String, CellStyle)> = Vec::new();
        let bold = CellStyle {
            bold: true,
            ..CellStyle::default()
        };
        let dim = CellStyle {
            dim: true,
            ..CellStyle::default()
        };
        let default = CellStyle::default();
        let cyan = CellStyle {
            fg: Some(Color::Cyan),
            bold: true,
            ..CellStyle::default()
        };

        let commit = match self.commits.get(self.selected) {
            Some(c) => c,
            None => return lines,
        };

        // Header
        lines.push((format!("Commit {}", commit.full_hash), cyan));

        if let Some(ref detail) = self.detail {
            lines.push((
                format!("Author: {} <{}>", detail.author, detail.author_email),
                bold.clone(),
            ));
            lines.push((format!("Date:   {}", detail.date), dim.clone()));
            lines.push((String::new(), default.clone()));

            // Full commit message
            for msg_line in detail.message.lines() {
                lines.push((msg_line.to_string(), default.clone()));
            }
            lines.push((String::new(), default.clone()));

            // Files changed
            if !detail.files.is_empty() {
                lines.push((
                    format!("Files changed ({}):", detail.files.len()),
                    bold.clone(),
                ));
                for file in &detail.files {
                    let status_color = match file.status {
                        'A' => Color::Green,
                        'D' => Color::Red,
                        'M' => Color::Yellow,
                        'R' => Color::Cyan,
                        _ => Color::White,
                    };
                    lines.push((
                        format!("  {} {}", file.status, file.path),
                        CellStyle {
                            fg: Some(status_color),
                            ..CellStyle::default()
                        },
                    ));
                }
                lines.push((String::new(), default.clone()));
            }

            // Diff
            for diff_line in &detail.diff_lines {
                lines.push((diff_line.clone(), diff_line_style(diff_line)));
            }
        } else {
            lines.push((String::new(), default.clone()));
            lines.push(("Loading...".to_string(), dim));
        }

        lines
    }
}

fn diff_line_style(line: &str) -> CellStyle {
    if line.starts_with('+') {
        CellStyle {
            fg: Some(Color::Green),
            ..CellStyle::default()
        }
    } else if line.starts_with('-') {
        CellStyle {
            fg: Some(Color::Red),
            ..CellStyle::default()
        }
    } else if line.starts_with("@@") {
        CellStyle {
            fg: Some(Color::Cyan),
            ..CellStyle::default()
        }
    } else if line.starts_with("diff ") || line.starts_with("index ") {
        CellStyle {
            bold: true,
            ..CellStyle::default()
        }
    } else {
        CellStyle {
            dim: true,
            ..CellStyle::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::commit_log_runtime::{CommitDetail, CommitFileEntry};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn shift_key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::SHIFT)
    }

    fn ctrl_key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    fn test_view() -> CommitLogView {
        CommitLogView {
            project_root: PathBuf::from("."),
            branch: "main".to_string(),
            commits: vec![
                CommitEntry {
                    hash: "abc1234".to_string(),
                    full_hash: "abc1234def5678".to_string(),
                    author: "Test".to_string(),
                    date: "2 days ago".to_string(),
                    message: "Fix bug in parser".to_string(),
                },
                CommitEntry {
                    hash: "def5678".to_string(),
                    full_hash: "def5678ghi9012".to_string(),
                    author: "Test".to_string(),
                    date: "3 days ago".to_string(),
                    message: "Add new feature".to_string(),
                },
                CommitEntry {
                    hash: "ghi9012".to_string(),
                    full_hash: "ghi9012jkl3456".to_string(),
                    author: "Test".to_string(),
                    date: "5 days ago".to_string(),
                    message: "Refactor module".to_string(),
                },
            ],
            selected: 0,
            scroll_offset: 0,
            view_mode: ViewMode::List,
            detail: None,
            detail_scroll: 0,
            detail_horizontal_scroll: 0,
            diff_lines: vec![
                "diff --git a/src/foo.rs b/src/foo.rs".to_string(),
                "@@ -1,3 +1,5 @@".to_string(),
                " context".to_string(),
                "+added".to_string(),
                "-removed".to_string(),
            ],
            diff_scroll: 0,
            diff_horizontal_scroll: 0,
            diff_loading: false,
            find_active: false,
            find_input: String::new(),
            runtime_tx: None,
            loading: false,
            has_more: false,
            message: None,
        }
    }

    #[test]
    fn move_down_up() {
        let mut view = test_view();
        assert_eq!(view.selected, 0);
        view.move_down();
        assert_eq!(view.selected, 1);
        view.move_down();
        assert_eq!(view.selected, 2);
        view.move_down();
        assert_eq!(view.selected, 2); // can't go past end
        view.move_up();
        assert_eq!(view.selected, 1);
    }

    #[test]
    fn key_j_moves_down() {
        let mut view = test_view();
        view.handle_key(key(KeyCode::Char('j')));
        assert_eq!(view.selected, 1);
    }

    #[test]
    fn key_k_moves_up() {
        let mut view = test_view();
        view.selected = 2;
        view.handle_key(key(KeyCode::Char('k')));
        assert_eq!(view.selected, 1);
    }

    #[test]
    fn key_q_closes() {
        let mut view = test_view();
        let result = view.handle_key(key(KeyCode::Char('q')));
        assert!(matches!(
            result,
            EventResult::Action(Action::Ui(UiAction::CloseCommitLog))
        ));
    }

    #[test]
    fn key_esc_closes() {
        let mut view = test_view();
        let result = view.handle_key(key(KeyCode::Esc));
        assert!(matches!(
            result,
            EventResult::Action(Action::Ui(UiAction::CloseCommitLog))
        ));
    }

    #[test]
    fn enter_opens_commit_diff_view() {
        let mut view = test_view();
        let result = view.handle_key(key(KeyCode::Enter));
        assert!(matches!(result, EventResult::Action(_)));
    }

    #[test]
    fn exit_detail_mode_with_esc() {
        let mut view = test_view();
        view.view_mode = ViewMode::Detail;
        view.handle_key(key(KeyCode::Esc));
        assert_eq!(view.view_mode, ViewMode::List);
    }

    #[test]
    fn exit_detail_mode_with_backspace() {
        let mut view = test_view();
        view.view_mode = ViewMode::Detail;
        view.handle_key(key(KeyCode::Backspace));
        assert_eq!(view.view_mode, ViewMode::List);
    }

    #[test]
    fn shift_j_scrolls_diff_down() {
        let mut view = test_view();
        view.handle_key(shift_key('J'));
        assert_eq!(view.diff_scroll, 1);
    }

    #[test]
    fn shift_k_scrolls_diff_up() {
        let mut view = test_view();
        view.diff_scroll = 3;
        view.handle_key(shift_key('K'));
        assert_eq!(view.diff_scroll, 2);
    }

    #[test]
    fn find_mode() {
        let mut view = test_view();
        view.handle_key(key(KeyCode::Char('/')));
        assert!(view.find_active);
        view.handle_key(key(KeyCode::Char('b')));
        assert_eq!(view.find_input, "b");
        view.handle_key(key(KeyCode::Esc));
        assert!(!view.find_active);
    }

    #[test]
    fn ctrl_q_closes() {
        let mut view = test_view();
        let result = view.handle_key(ctrl_key('q'));
        assert!(matches!(
            result,
            EventResult::Action(Action::Ui(UiAction::CloseCommitLog))
        ));
    }

    #[test]
    fn render_overlay_does_not_panic() {
        let mut view = test_view();
        let mut surface = Surface::new(120, 40);
        view.render_overlay(&mut surface);
    }

    #[test]
    fn render_detail_does_not_panic() {
        let mut view = test_view();
        view.view_mode = ViewMode::Detail;
        view.detail = Some(CommitDetail {
            full_hash: "abc1234def5678".to_string(),
            author: "Test".to_string(),
            author_email: "test@example.com".to_string(),
            date: "2 days ago".to_string(),
            message: "Fix bug\n\nDetailed description".to_string(),
            files: vec![CommitFileEntry {
                path: "src/foo.rs".to_string(),
                status: 'M',
            }],
            diff_lines: vec!["+added".to_string(), "-removed".to_string()],
        });
        let mut surface = Surface::new(120, 40);
        view.render_overlay(&mut surface);
    }

    #[test]
    fn apply_page_loaded() {
        let mut view = test_view();
        view.commits.clear();
        view.loading = true;
        view.message = Some("Loading commits...".to_string());
        view.apply_event(CommitLogEvent::PageLoaded {
            commits: vec![CommitEntry {
                hash: "aaa".to_string(),
                full_hash: "aaabbb".to_string(),
                author: "X".to_string(),
                date: "1d".to_string(),
                message: "msg".to_string(),
            }],
            has_more: false,
            is_append: false,
        });
        assert_eq!(view.commits.len(), 1);
        assert!(!view.loading);
        assert!(view.message.is_none());
    }

    #[test]
    fn apply_page_append() {
        let mut view = test_view();
        let initial_len = view.commits.len();
        view.apply_event(CommitLogEvent::PageLoaded {
            commits: vec![CommitEntry {
                hash: "new".to_string(),
                full_hash: "newww".to_string(),
                author: "X".to_string(),
                date: "1d".to_string(),
                message: "appended".to_string(),
            }],
            has_more: false,
            is_append: true,
        });
        assert_eq!(view.commits.len(), initial_len + 1);
    }
}
