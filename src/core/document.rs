use ropey::Rope;
use crate::core_lib::ui::text::char_display_width;
use std::borrow::Cow;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::command::git::GitLineStatus;
use crate::core::buffer::{CharClass, EditEvent, char_class};
use crate::core::history::{EditRecord, History};

pub type DocumentId = usize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    pub anchor: usize,
    pub head: usize,
    pub cursor_display: SelectionCursorDisplay,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionCursorDisplay {
    TailOnForward,
    Head,
}

impl Selection {
    pub fn tail_on_forward(anchor: usize, head: usize) -> Self {
        Self {
            anchor,
            head,
            cursor_display: SelectionCursorDisplay::TailOnForward,
        }
    }

    pub fn head(anchor: usize, head: usize) -> Self {
        Self {
            anchor,
            head,
            cursor_display: SelectionCursorDisplay::Head,
        }
    }
}

/// A document is a file-backed (or scratch) editing unit.
///
/// Combines a text buffer (Rope + EditEvent recording) with cursor state,
/// scroll position, file path, dirty tracking, and undo/redo history.
pub struct Document {
    pub id: DocumentId,
    pub rope: Rope,
    /// Multiple cursors. The first cursor (index 0) is the "primary" cursor.
    /// Invariants: never empty, sorted by position when multiple cursors exist.
    pub cursors: Vec<usize>,
    pub scroll_offset: usize,
    pub horizontal_scroll_offset: usize,
    pub file_path: Option<PathBuf>,
    pub dirty: bool,
    pub pending_edits: Vec<EditEvent>,
    pub history: History,
    /// Selection for the primary cursor only
    pub selection: Option<Selection>,
    pub git_gutter: HashMap<usize, GitLineStatus>,
    cached_status_bar_path: String,
}

impl Document {
    fn normalize_newlines_for_insert(text: &str) -> Cow<'_, str> {
        if !text.as_bytes().contains(&b'\r') {
            return Cow::Borrowed(text);
        }

        let mut normalized = String::with_capacity(text.len());
        let mut chars = text.chars().peekable();
        while let Some(ch) = chars.next() {
            if ch == '\r' {
                if matches!(chars.peek(), Some('\n')) {
                    chars.next();
                }
                normalized.push('\n');
            } else {
                normalized.push(ch);
            }
        }
        Cow::Owned(normalized)
    }

    pub fn new_scratch(id: DocumentId) -> Self {
        Self {
            id,
            rope: Rope::new(),
            cursors: vec![0],
            scroll_offset: 0,
            horizontal_scroll_offset: 0,
            file_path: None,
            dirty: false,
            pending_edits: Vec::new(),
            history: History::new(),
            selection: None,
            git_gutter: HashMap::new(),
            cached_status_bar_path: "[scratch]".to_string(),
        }
    }

    pub fn from_file(id: DocumentId, path: &str) -> Self {
        let (rope, file_path) = if let Ok(contents) = fs::read_to_string(path) {
            (Rope::from_str(&contents), PathBuf::from(path))
        } else {
            (Rope::new(), PathBuf::from(path))
        };
        let cached_status_bar_path = Self::compute_status_bar_path(&Some(file_path.clone()));
        Self {
            id,
            rope,
            cursors: vec![0],
            scroll_offset: 0,
            horizontal_scroll_offset: 0,
            file_path: Some(file_path),
            dirty: false,
            pending_edits: Vec::new(),
            history: History::new(),
            selection: None,
            git_gutter: HashMap::new(),
            cached_status_bar_path,
        }
    }

    pub fn display_name(&self) -> String {
        self.file_path
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "[scratch]".to_string())
    }

    /// Returns a formatted path suitable for status bar display:
    /// - Git repos: "[repo_name] relative/path"
    /// - Non-git: Full path
    /// - Scratch: "[scratch]"
    pub fn status_bar_path(&self) -> &str {
        &self.cached_status_bar_path
    }

    /// Compute the status bar path (called once during document creation)
    fn compute_status_bar_path(file_path: &Option<PathBuf>) -> String {
        let Some(path) = file_path else {
            return "[scratch]".to_string();
        };

        // Try to get git repo info
        match Self::get_git_repo_info(path) {
            Some((repo_name, relative_path)) => {
                format!("[{}] {}", repo_name, relative_path)
            }
            None => {
                // Not in a git repo, show full path
                path.display().to_string()
            }
        }
    }

    /// Returns (repo_name, relative_path) if the file is in a git repo
    fn get_git_repo_info(file_path: &std::path::Path) -> Option<(String, String)> {
        use std::path::Path;
        use std::process::Command;

        // Get the directory containing the file
        let file_dir = file_path.parent()?;

        // Get git repo root
        let output = Command::new("git")
            .current_dir(file_dir)
            .args(["rev-parse", "--show-toplevel"])
            .output()
            .ok()?;

        if !output.status.success() {
            return None;
        }

        let repo_root_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let repo_root = Path::new(&repo_root_str);

        // Extract repo name from remote URL (preferred)
        let repo_name = Command::new("git")
            .current_dir(file_dir)
            .args(["config", "--get", "remote.origin.url"])
            .output()
            .ok()
            .and_then(|out| {
                if out.status.success() {
                    let remote = String::from_utf8_lossy(&out.stdout).trim().to_string();
                    Self::extract_repo_name_from_remote(&remote)
                } else {
                    None
                }
            })
            .or_else(|| {
                // Fallback: use the directory name of the repo root
                repo_root
                    .file_name()
                    .and_then(|n| n.to_str())
                    .map(|s| s.to_string())
            })?;

        // Compute relative path
        let relative_path = file_path
            .canonicalize()
            .ok()?
            .strip_prefix(repo_root)
            .ok()?
            .display()
            .to_string();

        Some((repo_name, relative_path))
    }

    /// Extract repository name from git remote URL
    /// Examples:
    ///   git@github.com:user/repo.git -> repo
    ///   https://github.com/user/repo.git -> repo
    ///   https://github.com/user/repo -> repo
    fn extract_repo_name_from_remote(remote: &str) -> Option<String> {
        let remote = remote.trim();

        // Extract the last component (repo name)
        let path_part = if remote.starts_with("git@github.com:") {
            remote.strip_prefix("git@github.com:")?
        } else if remote.starts_with("https://github.com/") {
            remote.strip_prefix("https://github.com/")?
        } else if remote.starts_with("http://github.com/") {
            remote.strip_prefix("http://github.com/")?
        } else if remote.starts_with("git@") {
            // Generic git SSH format: git@host:path
            remote.split(':').nth(1)?
        } else if remote.starts_with("https://") || remote.starts_with("http://") {
            // Generic HTTPS format
            remote.split('/').next_back()?
        } else {
            return None;
        };

        // Extract just the repo name (last part after /)
        let repo_name = path_part
            .trim_end_matches(".git")
            .split('/')
            .next_back()?
            .to_string();

        if repo_name.is_empty() {
            None
        } else {
            Some(repo_name)
        }
    }

    pub fn save(&mut self) -> Result<String, String> {
        let path = match &self.file_path {
            Some(p) => p.clone(),
            None => return Err("No file path".to_string()),
        };
        let contents = self.rope.to_string();
        fs::write(&path, &contents).map_err(|e| e.to_string())?;
        self.dirty = false;
        Ok(format!("Wrote {}", path.display()))
    }

    pub fn save_as(&mut self, path: &Path) -> Result<String, String> {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }

        let contents = self.rope.to_string();
        fs::write(path, &contents).map_err(|e| e.to_string())?;

        let normalized = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        self.file_path = Some(normalized.clone());
        self.cached_status_bar_path = Self::compute_status_bar_path(&self.file_path);
        self.dirty = false;
        Ok(format!("Wrote {}", normalized.display()))
    }

    pub fn rename_file(&mut self, new_path: &Path) -> Result<String, String> {
        let old_path = self
            .file_path
            .clone()
            .ok_or_else(|| "No file path to rename".to_string())?;

        if let Some(parent) = new_path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }

        fs::rename(&old_path, new_path).map_err(|e| e.to_string())?;

        let normalized = fs::canonicalize(new_path).unwrap_or_else(|_| new_path.to_path_buf());
        self.file_path = Some(normalized.clone());
        self.cached_status_bar_path = Self::compute_status_bar_path(&self.file_path);
        Ok(format!("Renamed to {}", normalized.display()))
    }

    pub fn reload_from_disk(&mut self) -> Result<String, String> {
        let path = match &self.file_path {
            Some(p) => p.clone(),
            None => return Err("No file path to reload".to_string()),
        };
        let contents = fs::read_to_string(&path).map_err(|e| e.to_string())?;
        let old_cursor = self.cursors[0];
        self.rope = Rope::from_str(&contents);
        // Preserve cursor position if still valid, reset to single cursor
        self.cursors = vec![old_cursor.min(self.rope.len_chars())];
        self.dirty = false;
        self.pending_edits.clear();
        self.history = History::new();
        self.selection = None;
        Ok(format!("Reloaded {}", path.display()))
    }

    pub fn has_selection(&self) -> bool {
        self.selection.is_some()
    }

    pub fn selection_anchor(&self) -> Option<usize> {
        self.selection.map(|s| s.anchor)
    }

    // -------------------------------------------------------
    // Multi-cursor helpers
    // -------------------------------------------------------

    /// Returns the primary cursor position (first cursor)
    #[inline]
    pub fn cursor(&self) -> usize {
        self.cursors[0]
    }

    /// Sets the primary cursor position
    #[inline]
    pub fn set_cursor(&mut self, pos: usize) {
        self.cursors[0] = pos;
    }

    /// Number of active cursors
    pub fn cursor_count(&self) -> usize {
        self.cursors.len()
    }

    /// Check if multiple cursors are active
    pub fn has_multiple_cursors(&self) -> bool {
        self.cursors.len() > 1
    }

    /// Sort cursors by position and remove duplicates.
    /// Keeps the primary cursor at index 0.
    fn sort_and_dedup_cursors(&mut self) {
        if self.cursors.len() <= 1 {
            return;
        }
        let primary = self.cursors[0];
        self.cursors.sort_unstable();
        self.cursors.dedup();
        // Restore primary to front
        if let Some(idx) = self.cursors.iter().position(|&c| c == primary)
            && idx != 0
        {
            self.cursors.swap(0, idx);
        }
    }

    /// Add a cursor at the given char offset.
    /// Returns true if a new cursor was added.
    pub fn add_cursor_at(&mut self, pos: usize) -> bool {
        let pos = pos.min(self.rope.len_chars());
        if self.cursors.contains(&pos) {
            return false;
        }
        self.cursors.push(pos);
        self.sort_and_dedup_cursors();
        true
    }

    /// Add a cursor on the line above at the same column (best effort).
    /// Uses the primary cursor's column but adds above the topmost existing cursor.
    /// Returns true if a cursor was added.
    pub fn add_cursor_above(&mut self) -> bool {
        // Use primary cursor's column
        let primary_pos = self.cursors[0];
        let primary_line = self.rope.char_to_line(primary_pos);
        let col = primary_pos - self.rope.line_to_char(primary_line);

        // Find the topmost (smallest line number) cursor
        let top_cursor = *self.cursors.iter().min().unwrap();
        let top_line = self.rope.char_to_line(top_cursor);

        if top_line == 0 {
            return false;
        }

        let target_line = top_line - 1;
        let target_line_start = self.rope.line_to_char(target_line);
        let target_line_len = self.line_len(target_line);
        let clamped_col = col.min(target_line_len);
        let new_pos = target_line_start + clamped_col;

        // Don't add if already exists
        if self.cursors.contains(&new_pos) {
            return false;
        }

        self.cursors.push(new_pos);
        self.sort_and_dedup_cursors();
        true
    }

    /// Add a cursor on the line below at the same column (best effort).
    /// Uses the primary cursor's column but adds below the bottommost existing cursor.
    /// Returns true if a cursor was added.
    pub fn add_cursor_below(&mut self) -> bool {
        // Use primary cursor's column
        let primary_pos = self.cursors[0];
        let primary_line = self.rope.char_to_line(primary_pos);
        let col = primary_pos - self.rope.line_to_char(primary_line);

        // Find the bottommost (largest line number) cursor
        let bottom_cursor = *self.cursors.iter().max().unwrap();
        let bottom_line = self.rope.char_to_line(bottom_cursor);

        if bottom_line + 1 >= self.rope.len_lines() {
            return false;
        }

        let target_line = bottom_line + 1;
        let target_line_start = self.rope.line_to_char(target_line);
        let target_line_len = self.line_len(target_line);
        let clamped_col = col.min(target_line_len);
        let new_pos = target_line_start + clamped_col;

        // Don't add if already exists
        if self.cursors.contains(&new_pos) {
            return false;
        }

        self.cursors.push(new_pos);
        self.sort_and_dedup_cursors();
        true
    }

    /// Add cursors from the topmost existing cursor up to the first line,
    /// one cursor per line at the primary cursor's column.
    pub fn add_cursors_to_top(&mut self) {
        let primary_pos = self.cursors[0];
        let primary_line = self.rope.char_to_line(primary_pos);
        let col = primary_pos - self.rope.line_to_char(primary_line);

        let top_cursor = *self.cursors.iter().min().unwrap();
        let top_line = self.rope.char_to_line(top_cursor);

        for line in (0..top_line).rev() {
            let line_start = self.rope.line_to_char(line);
            let clamped_col = col.min(self.line_len(line));
            let new_pos = line_start + clamped_col;
            if !self.cursors.contains(&new_pos) {
                self.cursors.push(new_pos);
            }
        }
        self.sort_and_dedup_cursors();
    }

    /// Add cursors from the bottommost existing cursor down to the last line,
    /// one cursor per line at the primary cursor's column.
    pub fn add_cursors_to_bottom(&mut self) {
        let primary_pos = self.cursors[0];
        let primary_line = self.rope.char_to_line(primary_pos);
        let col = primary_pos - self.rope.line_to_char(primary_line);

        let bottom_cursor = *self.cursors.iter().max().unwrap();
        let bottom_line = self.rope.char_to_line(bottom_cursor);
        let last_line = self.rope.len_lines().saturating_sub(1);

        for line in (bottom_line + 1)..=last_line {
            let line_start = self.rope.line_to_char(line);
            let clamped_col = col.min(self.line_len(line));
            let new_pos = line_start + clamped_col;
            if !self.cursors.contains(&new_pos) {
                self.cursors.push(new_pos);
            }
        }
        self.sort_and_dedup_cursors();
    }

    /// Remove all secondary cursors, keeping only the primary cursor.
    pub fn remove_secondary_cursors(&mut self) {
        self.cursors.truncate(1);
    }

    pub fn display_cursor(&self) -> usize {
        if let Some(selection) = self.selection {
            if matches!(
                selection.cursor_display,
                SelectionCursorDisplay::TailOnForward
            ) && selection.head > selection.anchor
            {
                return selection.head.saturating_sub(1);
            }
            return selection.head;
        }
        self.cursors[0]
    }

    fn sync_selection_head(&mut self) {
        if let Some(mut sel) = self.selection {
            sel.head = self.cursors[0];
            self.selection = Some(sel);
        }
    }

    pub fn cursor_line(&self) -> usize {
        self.rope.char_to_line(self.cursors[0])
    }

    pub fn cursor_col(&self) -> usize {
        let line_start = self.rope.line_to_char(self.cursor_line());
        self.cursors[0] - line_start
    }

    pub fn display_cursor_line(&self) -> usize {
        self.rope.char_to_line(self.display_cursor())
    }

    pub fn display_cursor_col(&self) -> usize {
        let line_start = self.rope.line_to_char(self.display_cursor_line());
        self.display_cursor() - line_start
    }

    pub fn display_cursor_display_col(&self) -> usize {
        let line = self.display_cursor_line();
        let target_char_col = self.display_cursor_col();
        let line_slice = self.rope.line(line);
        let mut col = 0usize;
        for idx in 0..target_char_col.min(line_slice.len_chars()) {
            let ch = line_slice.char(idx);
            if ch == '\n' {
                break;
            }
            col += char_display_width(ch);
        }
        col
    }

    pub fn cursor_position_utf16(&self) -> (u32, u32) {
        let line = self.cursor_line();
        let col_char = self.cursor_col();
        let col_utf16 = self.char_col_to_utf16(line, col_char) as u32;
        (line as u32, col_utf16)
    }

    pub fn char_col_to_utf16(&self, line: usize, char_col: usize) -> usize {
        let line_slice = self.rope.line(line);
        let text = line_slice.to_string();
        let line_text = text.trim_end_matches('\n');
        line_text
            .chars()
            .take(char_col.min(line_text.chars().count()))
            .map(char::len_utf16)
            .sum()
    }

    pub fn utf16_to_char_col(&self, line: usize, utf16_col: usize) -> usize {
        let line_slice = self.rope.line(line);
        let text = line_slice.to_string();
        let line_text = text.trim_end_matches('\n');
        let mut units = 0usize;
        let mut chars = 0usize;
        for ch in line_text.chars() {
            let next = units + ch.len_utf16();
            if next > utf16_col {
                break;
            }
            units = next;
            chars += 1;
        }
        chars
    }

    pub fn set_cursor_line_char(&mut self, line: usize, char_col: usize) {
        let total_lines = self.rope.len_lines();
        let line = line.min(total_lines.saturating_sub(1));
        let line_start = self.rope.line_to_char(line);
        let line_len = self.line_len(line);
        self.cursors[0] = line_start + char_col.min(line_len);
        self.sync_selection_head();
    }

    pub fn ensure_cursor_visible(&mut self, view_height: usize) {
        // Scroll is based on primary cursor
        let line = self.cursor_line();
        if line < self.scroll_offset {
            self.scroll_offset = line;
        } else if line >= self.scroll_offset + view_height {
            self.scroll_offset = line - view_height + 1;
        }
    }

    pub fn ensure_cursor_visible_with_horizontal(
        &mut self,
        view_height: usize,
        text_width: usize,
        margin: usize,
    ) {
        self.ensure_cursor_visible(view_height);

        if text_width == 0 {
            self.horizontal_scroll_offset = 0;
            return;
        }

        let line = self.display_cursor_line();
        let line_width = self.line_display_width(line);
        if line_width <= text_width {
            self.horizontal_scroll_offset = 0;
            return;
        }

        let cursor_col = self.display_cursor_display_col();
        let effective_margin = margin.min(text_width.saturating_sub(1));
        let right_margin_span = text_width
            .saturating_sub(1)
            .saturating_sub(effective_margin);

        let left_trigger = self.horizontal_scroll_offset + effective_margin;
        if cursor_col < left_trigger {
            self.horizontal_scroll_offset = cursor_col.saturating_sub(effective_margin);
        } else {
            let right_trigger = self.horizontal_scroll_offset + right_margin_span;
            if cursor_col > right_trigger {
                self.horizontal_scroll_offset = cursor_col.saturating_sub(right_margin_span);
            }
        }

        let max_offset = line_width.saturating_sub(text_width);
        self.horizontal_scroll_offset = self.horizontal_scroll_offset.min(max_offset);
    }

    /// Scroll the viewport by `delta` lines without moving the cursor,
    /// unless the cursor would fall outside the visible area.
    /// Positive delta scrolls down (content moves up), negative scrolls up.
    pub fn scroll_viewport(&mut self, delta: isize, view_height: usize) {
        let total_lines = self.rope.len_lines();
        let max_scroll = total_lines.saturating_sub(1);

        let new_scroll = if delta >= 0 {
            self.scroll_offset
                .saturating_add(delta as usize)
                .min(max_scroll)
        } else {
            self.scroll_offset.saturating_sub((-delta) as usize)
        };
        self.scroll_offset = new_scroll;

        // Clamp primary cursor to stay within the visible viewport
        let cursor_line = self.cursor_line();
        if cursor_line < new_scroll {
            let target_line = new_scroll;
            let old_line_start = self.rope.line_to_char(cursor_line);
            let col = self.cursors[0] - old_line_start;
            let line_start = self.rope.line_to_char(target_line);
            let line_len = self.line_len(target_line);
            self.cursors[0] = line_start + col.min(line_len);
        } else if cursor_line >= new_scroll + view_height {
            let target_line = (new_scroll + view_height - 1).min(total_lines.saturating_sub(1));
            let old_line_start = self.rope.line_to_char(cursor_line);
            let col = self.cursors[0] - old_line_start;
            let line_start = self.rope.line_to_char(target_line);
            let line_len = self.line_len(target_line);
            self.cursors[0] = line_start + col.min(line_len);
        }
    }

    // -------------------------------------------------------
    // Cursor movement (no undo tracking needed)
    // -------------------------------------------------------

    pub fn move_right(&mut self) {
        let len = self.rope.len_chars();
        for cursor in &mut self.cursors {
            if *cursor < len {
                *cursor += 1;
            }
        }
        self.sort_and_dedup_cursors();
        self.sync_selection_head();
    }

    pub fn move_left(&mut self) {
        for cursor in &mut self.cursors {
            if *cursor > 0 {
                *cursor -= 1;
            }
        }
        self.sort_and_dedup_cursors();
        self.sync_selection_head();
    }

    pub fn move_down(&mut self) {
        let total_lines = self.rope.len_lines();
        let new_positions: Vec<usize> = self
            .cursors
            .iter()
            .map(|&cursor| {
                let line = self.rope.char_to_line(cursor);
                let line_start = self.rope.line_to_char(line);
                let col = cursor - line_start;
                if line + 1 < total_lines {
                    let next_line_start = self.rope.line_to_char(line + 1);
                    let next_line_len = self.line_len(line + 1);
                    next_line_start + col.min(next_line_len)
                } else {
                    cursor
                }
            })
            .collect();
        self.cursors = new_positions;
        self.sort_and_dedup_cursors();
        self.sync_selection_head();
    }

    pub fn move_up(&mut self) {
        let new_positions: Vec<usize> = self
            .cursors
            .iter()
            .map(|&cursor| {
                let line = self.rope.char_to_line(cursor);
                let line_start = self.rope.line_to_char(line);
                let col = cursor - line_start;
                if line > 0 {
                    let prev_line_start = self.rope.line_to_char(line - 1);
                    let prev_line_len = self.line_len(line - 1);
                    prev_line_start + col.min(prev_line_len)
                } else {
                    cursor
                }
            })
            .collect();
        self.cursors = new_positions;
        self.sort_and_dedup_cursors();
        self.sync_selection_head();
    }

    pub fn move_to_file_start(&mut self) {
        // All cursors collapse to position 0
        self.cursors = vec![0];
        self.sync_selection_head();
    }

    pub fn move_to_file_end(&mut self) {
        // All cursors collapse to end
        self.cursors = vec![self.rope.len_chars()];
        self.sync_selection_head();
    }

    pub fn move_to_line_start(&mut self) {
        let new_positions: Vec<usize> = self
            .cursors
            .iter()
            .map(|&cursor| {
                let line = self.rope.char_to_line(cursor);
                self.rope.line_to_char(line)
            })
            .collect();
        self.cursors = new_positions;
        self.sort_and_dedup_cursors();
        self.sync_selection_head();
    }

    pub fn move_to_line_end(&mut self) {
        let new_positions: Vec<usize> = self
            .cursors
            .iter()
            .map(|&cursor| {
                let line = self.rope.char_to_line(cursor);
                let line_start = self.rope.line_to_char(line);
                line_start + self.line_len(line)
            })
            .collect();
        self.cursors = new_positions;
        self.sort_and_dedup_cursors();
        self.sync_selection_head();
    }

    pub fn move_to_line_first_non_whitespace(&mut self) {
        let new_positions: Vec<usize> = self
            .cursors
            .iter()
            .map(|&cursor| {
                let line = self.rope.char_to_line(cursor);
                let line_start = self.rope.line_to_char(line);
                let line_slice = self.rope.line(line);
                let mut offset = 0usize;
                while offset < line_slice.len_chars() {
                    let ch = line_slice.char(offset);
                    if ch == '\n' || !ch.is_whitespace() {
                        break;
                    }
                    offset += 1;
                }
                line_start + offset
            })
            .collect();
        self.cursors = new_positions;
        self.sort_and_dedup_cursors();
        self.sync_selection_head();
    }

    fn motion_class(c: char, long_word: bool) -> CharClass {
        if long_word {
            if c.is_whitespace() {
                CharClass::Whitespace
            } else {
                CharClass::Word
            }
        } else {
            char_class(c)
        }
    }

    fn is_inline_whitespace(c: char) -> bool {
        c.is_whitespace() && c != '\n' && c != '\r'
    }

    fn word_forward_pos(&self, pos: usize, long_word: bool) -> usize {
        let len = self.rope.len_chars();
        if pos >= len {
            return pos;
        }

        let mut new_pos = pos;

        // Keep block-cursor style behavior around line endings:
        // when "on" '\n' (displayed as previous char), the next `w`
        // should enter the next line's indentation run.
        let ch = self.rope.char(new_pos);
        if ch == '\n' || ch == '\r' {
            new_pos += 1;
            while new_pos < len && Self::is_inline_whitespace(self.rope.char(new_pos)) {
                new_pos += 1;
            }
            return new_pos;
        }

        let start_class = Self::motion_class(self.rope.char(new_pos), long_word);
        if start_class != CharClass::Whitespace {
            while new_pos < len
                && Self::motion_class(self.rope.char(new_pos), long_word) == start_class
            {
                new_pos += 1;
            }
        }
        while new_pos < len && Self::is_inline_whitespace(self.rope.char(new_pos)) {
            new_pos += 1;
        }
        new_pos
    }

    fn move_word_forward_impl(&mut self, long_word: bool) {
        let new_positions: Vec<usize> = self
            .cursors
            .iter()
            .map(|&pos| self.word_forward_pos(pos, long_word))
            .collect();
        self.cursors = new_positions;
        self.sort_and_dedup_cursors();
        self.sync_selection_head();
    }

    fn word_forward_end_pos(&self, pos: usize, long_word: bool, in_selection: bool) -> usize {
        let len = self.rope.len_chars();
        if pos + 1 >= len {
            return len.saturating_sub(1);
        }
        let mut new_pos = pos + 1;
        while new_pos < len
            && Self::motion_class(self.rope.char(new_pos), long_word) == CharClass::Whitespace
        {
            new_pos += 1;
        }
        if new_pos >= len {
            return len.saturating_sub(1);
        }
        let cls = Self::motion_class(self.rope.char(new_pos), long_word);
        while new_pos + 1 < len && Self::motion_class(self.rope.char(new_pos + 1), long_word) == cls
        {
            new_pos += 1;
        }
        // In range-based (selection) movement, keep the head one-past-the-end so the
        // displayed block cursor lands on the actual word-end character.
        if in_selection && new_pos < len {
            new_pos += 1;
        }
        new_pos
    }

    fn move_word_forward_end_impl(&mut self, long_word: bool) {
        let in_selection = self.selection.is_some();
        let new_positions: Vec<usize> = self
            .cursors
            .iter()
            .map(|&pos| self.word_forward_end_pos(pos, long_word, in_selection))
            .collect();
        self.cursors = new_positions;
        self.sort_and_dedup_cursors();
        self.sync_selection_head();
    }

    fn word_backward_pos(&self, pos: usize, long_word: bool) -> usize {
        if pos == 0 {
            return 0;
        }
        let mut new_pos = pos - 1;
        while new_pos > 0
            && Self::motion_class(self.rope.char(new_pos), long_word) == CharClass::Whitespace
        {
            new_pos -= 1;
        }
        let cls = Self::motion_class(self.rope.char(new_pos), long_word);
        while new_pos > 0 && Self::motion_class(self.rope.char(new_pos - 1), long_word) == cls {
            new_pos -= 1;
        }
        new_pos
    }

    fn move_word_backward_impl(&mut self, long_word: bool) {
        let new_positions: Vec<usize> = self
            .cursors
            .iter()
            .map(|&pos| self.word_backward_pos(pos, long_word))
            .collect();
        self.cursors = new_positions;
        self.sort_and_dedup_cursors();
        self.sync_selection_head();
    }

    pub fn move_word_forward(&mut self) {
        self.move_word_forward_impl(false);
    }

    pub fn move_word_forward_end(&mut self) {
        self.move_word_forward_end_impl(false);
    }

    pub fn move_word_backward(&mut self) {
        self.move_word_backward_impl(false);
    }

    pub fn move_long_word_forward(&mut self) {
        self.move_word_forward_impl(true);
    }

    pub fn move_long_word_forward_end(&mut self) {
        self.move_word_forward_end_impl(true);
    }

    pub fn move_long_word_backward(&mut self) {
        self.move_word_backward_impl(true);
    }

    fn ensure_anchor_for_extend(&mut self) {
        if self.selection.is_none() {
            self.set_anchor();
        }
    }

    fn ensure_anchor_for_shift_extend(&mut self) {
        if self.selection.is_none() {
            self.set_anchor_for_shift_extend();
            return;
        }
        if let Some(selection) = self.selection.as_mut() {
            selection.cursor_display = SelectionCursorDisplay::Head;
        }
    }

    pub fn extend_word_forward(&mut self) {
        self.ensure_anchor_for_extend();
        self.move_word_forward_impl(false);
    }

    pub fn extend_right(&mut self) {
        self.ensure_anchor_for_shift_extend();
        self.move_right();
    }

    pub fn extend_left(&mut self) {
        self.ensure_anchor_for_shift_extend();
        self.move_left();
    }

    pub fn extend_word_forward_shift(&mut self) {
        self.ensure_anchor_for_shift_extend();
        self.move_word_forward_impl(false);
    }

    pub fn extend_word_backward_shift(&mut self) {
        self.ensure_anchor_for_shift_extend();
        self.move_word_backward_impl(false);
    }

    pub fn extend_word_forward_end(&mut self) {
        self.ensure_anchor_for_extend();
        self.move_word_forward_end_impl(false);
    }

    pub fn extend_word_backward(&mut self) {
        self.ensure_anchor_for_extend();
        self.move_word_backward_impl(false);
    }

    pub fn extend_long_word_forward(&mut self) {
        self.ensure_anchor_for_extend();
        self.move_word_forward_impl(true);
    }

    pub fn extend_long_word_forward_end(&mut self) {
        self.ensure_anchor_for_extend();
        self.move_word_forward_end_impl(true);
    }

    pub fn extend_long_word_backward(&mut self) {
        self.ensure_anchor_for_extend();
        self.move_word_backward_impl(true);
    }

    pub fn current_line_is_empty(&self) -> bool {
        self.line_len(self.cursor_line()) == 0
    }

    pub fn indent_for_empty_line(&self) -> String {
        let line = self.cursor_line();
        let source_line = if line > 0 { line - 1 } else { line };
        let text = self.rope.line(source_line).to_string();
        text.chars()
            .take_while(|c| *c == ' ' || *c == '\t')
            .collect()
    }

    pub fn append_newline_at_eof(&mut self) {
        let end = self.rope.len_chars();
        self.insert_text_at(end, "\n");
    }

    // -------------------------------------------------------
    // Editing operations (with undo recording)
    // -------------------------------------------------------

    pub fn insert_char(&mut self, c: char) {
        // Multi-cursor insert: process from highest to lowest position
        let mut positions: Vec<usize> = self.cursors.clone();
        positions.sort_by(|a, b| b.cmp(a)); // Descending order

        let cursors_before = self.cursors.clone();
        let char_len = c.len_utf8();

        // Begin transaction to group all multi-cursor edits together
        // Only commit if we started the transaction (not if one was already open externally)
        let started_transaction = self.history.begin_transaction(&cursors_before);

        for pos in &positions {
            let byte_pos = self.rope.char_to_byte(*pos);
            let line = self.rope.char_to_line(*pos);
            let line_byte_start = self.rope.line_to_byte(line);
            let col_byte = byte_pos - line_byte_start;

            self.rope.insert_char(*pos, c);

            let new_line = if c == '\n' { line + 1 } else { line };
            let new_col_byte = if c == '\n' { 0 } else { col_byte + char_len };
            let edit_event = EditEvent {
                start_byte: byte_pos,
                old_end_byte: byte_pos,
                new_end_byte: byte_pos + char_len,
                start_position: (line, col_byte),
                old_end_position: (line, col_byte),
                new_end_position: (new_line, new_col_byte),
            };
            self.pending_edits.push(edit_event.clone());

            // Record history for undo - record ALL cursor edits
            self.history.record(
                EditRecord {
                    char_offset: *pos,
                    old_text: String::new(),
                    new_text: c.to_string(),
                    edit_event,
                },
                &cursors_before,
                &cursors_before, // Will be updated after cursor adjustment
            );
        }

        // Adjust all cursor positions: each cursor moves forward by 1
        // plus the number of cursors that were before it
        let original_positions: Vec<usize> = self.cursors.clone();
        for (i, cursor) in self.cursors.iter_mut().enumerate() {
            let cursors_before_count = original_positions
                .iter()
                .filter(|&&p| p < original_positions[i])
                .count();
            *cursor = original_positions[i] + 1 + cursors_before_count;
        }

        // Update the cursors_after and commit only if we started the transaction
        self.history.update_cursors_after(&self.cursors);
        if started_transaction {
            self.history.commit_transaction();
        }

        self.dirty = true;
        self.sort_and_dedup_cursors();
    }

    pub fn insert_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        let text = Self::normalize_newlines_for_insert(text);
        let text = text.as_ref();
        let char_count = text.chars().count();

        // Multi-cursor insert: process from highest to lowest position
        let mut positions: Vec<usize> = self.cursors.clone();
        positions.sort_by(|a, b| b.cmp(a)); // Descending order

        let cursors_before = self.cursors.clone();

        // Begin transaction to group all multi-cursor edits together
        // Only commit if we started the transaction (not if one was already open externally)
        let started_transaction = self.history.begin_transaction(&cursors_before);

        for pos in &positions {
            let byte_pos = self.rope.char_to_byte(*pos);
            let line = self.rope.char_to_line(*pos);
            let line_byte_start = self.rope.line_to_byte(line);
            let col_byte = byte_pos - line_byte_start;

            self.rope.insert(*pos, text);

            let new_end_pos = self.compute_end_position(line, col_byte, text);
            let edit_event = EditEvent {
                start_byte: byte_pos,
                old_end_byte: byte_pos,
                new_end_byte: byte_pos + text.len(),
                start_position: (line, col_byte),
                old_end_position: (line, col_byte),
                new_end_position: new_end_pos,
            };
            self.pending_edits.push(edit_event.clone());

            // Record history for undo - record ALL cursor edits
            self.history.record(
                EditRecord {
                    char_offset: *pos,
                    old_text: String::new(),
                    new_text: text.to_string(),
                    edit_event,
                },
                &cursors_before,
                &cursors_before, // Will be updated after cursor adjustment
            );
        }

        // Adjust all cursor positions
        let original_positions: Vec<usize> = self.cursors.clone();
        for (i, cursor) in self.cursors.iter_mut().enumerate() {
            let cursors_before_count = original_positions
                .iter()
                .filter(|&&p| p < original_positions[i])
                .count();
            *cursor = original_positions[i] + char_count + (cursors_before_count * char_count);
        }

        // Update the cursors_after and commit only if we started the transaction
        self.history.update_cursors_after(&self.cursors);
        if started_transaction {
            self.history.commit_transaction();
        }

        self.dirty = true;
        self.sort_and_dedup_cursors();
    }

    #[cfg(test)]
    pub fn insert_newline(&mut self) {
        // Use insert_char for newline to get multi-cursor support
        self.insert_char('\n');
    }

    pub fn delete_forward(&mut self) {
        let len = self.rope.len_chars();
        // Multi-cursor delete: process from highest to lowest position
        let mut positions: Vec<usize> =
            self.cursors.iter().filter(|&&p| p < len).cloned().collect();
        if positions.is_empty() {
            return;
        }
        positions.sort_by(|a, b| b.cmp(a)); // Descending order

        let cursors_before = self.cursors.clone();

        // Begin transaction to group all multi-cursor edits together
        // Only commit if we started the transaction (not if one was already open externally)
        let started_transaction = self.history.begin_transaction(&cursors_before);

        for pos in &positions {
            let byte_pos = self.rope.char_to_byte(*pos);
            let line = self.rope.char_to_line(*pos);
            let line_byte_start = self.rope.line_to_byte(line);
            let col_byte = byte_pos - line_byte_start;
            let ch = self.rope.char(*pos);
            let char_len = ch.len_utf8();

            let old_end_line = if ch == '\n' { line + 1 } else { line };
            let old_end_col = if ch == '\n' { 0 } else { col_byte + char_len };

            self.rope.remove(*pos..*pos + 1);

            let edit_event = EditEvent {
                start_byte: byte_pos,
                old_end_byte: byte_pos + char_len,
                new_end_byte: byte_pos,
                start_position: (line, col_byte),
                old_end_position: (old_end_line, old_end_col),
                new_end_position: (line, col_byte),
            };
            self.pending_edits.push(edit_event.clone());

            // Record history for undo - record ALL cursor edits
            self.history.record(
                EditRecord {
                    char_offset: *pos,
                    old_text: ch.to_string(),
                    new_text: String::new(),
                    edit_event,
                },
                &cursors_before,
                &cursors_before, // Will be updated after cursor adjustment
            );
        }

        // Adjust cursor positions: cursors after deleted positions shift back
        let original_positions: Vec<usize> = self.cursors.clone();
        for (i, cursor) in self.cursors.iter_mut().enumerate() {
            let deleted_before = positions
                .iter()
                .filter(|&&p| p < original_positions[i])
                .count();
            *cursor = original_positions[i].saturating_sub(deleted_before);
        }

        // Update the cursors_after and commit only if we started the transaction
        self.history.update_cursors_after(&self.cursors);
        if started_transaction {
            self.history.commit_transaction();
        }

        self.dirty = true;
        self.sort_and_dedup_cursors();
    }

    pub fn delete_backward(&mut self) {
        // Multi-cursor delete: process from highest to lowest position
        // Each cursor deletes the character before it
        let mut positions: Vec<usize> = self.cursors.iter().filter(|&&p| p > 0).cloned().collect();
        if positions.is_empty() {
            return;
        }
        positions.sort_by(|a, b| b.cmp(a)); // Descending order

        let cursors_before = self.cursors.clone();

        // Begin transaction to group all multi-cursor edits together
        // Only commit if we started the transaction (not if one was already open externally)
        let started_transaction = self.history.begin_transaction(&cursors_before);

        for pos in &positions {
            let delete_pos = *pos - 1;
            let byte_pos = self.rope.char_to_byte(delete_pos);
            let line = self.rope.char_to_line(delete_pos);
            let line_byte_start = self.rope.line_to_byte(line);
            let col_byte = byte_pos - line_byte_start;
            let ch = self.rope.char(delete_pos);
            let char_len = ch.len_utf8();

            let old_end_line = if ch == '\n' { line + 1 } else { line };
            let old_end_col = if ch == '\n' { 0 } else { col_byte + char_len };

            self.rope.remove(delete_pos..delete_pos + 1);

            let edit_event = EditEvent {
                start_byte: byte_pos,
                old_end_byte: byte_pos + char_len,
                new_end_byte: byte_pos,
                start_position: (line, col_byte),
                old_end_position: (old_end_line, old_end_col),
                new_end_position: (line, col_byte),
            };
            self.pending_edits.push(edit_event.clone());

            // Record history for undo - record ALL cursor edits
            self.history.record(
                EditRecord {
                    char_offset: delete_pos,
                    old_text: ch.to_string(),
                    new_text: String::new(),
                    edit_event,
                },
                &cursors_before,
                &cursors_before, // Will be updated after cursor adjustment
            );
        }

        // Adjust cursor positions: each cursor moves back by 1 plus the number of
        // deletions that happened before it
        let original_positions: Vec<usize> = self.cursors.clone();
        for (i, cursor) in self.cursors.iter_mut().enumerate() {
            if original_positions[i] > 0 {
                let deletions_before = positions
                    .iter()
                    .filter(|&&p| p <= original_positions[i])
                    .count();
                *cursor = original_positions[i].saturating_sub(deletions_before);
            }
        }

        // Update the cursors_after and commit only if we started the transaction
        self.history.update_cursors_after(&self.cursors);
        if started_transaction {
            self.history.commit_transaction();
        }

        self.dirty = true;
        self.sort_and_dedup_cursors();
    }

    pub fn kill_line(&mut self) {
        // kill_line operates on primary cursor only for now
        let cursors_before = self.cursors.clone();
        let line = self.cursor_line();
        let line_start = self.rope.line_to_char(line);
        let line_len = self.line_len(line);
        let line_end = line_start + line_len;

        if self.cursors[0] == line_end {
            if self.cursors[0] < self.rope.len_chars() {
                // Deleting the newline character
                let byte_pos = self.rope.char_to_byte(self.cursors[0]);
                let line_byte_start = self.rope.line_to_byte(line);
                let col_byte = byte_pos - line_byte_start;

                self.rope.remove(self.cursors[0]..self.cursors[0] + 1);
                self.dirty = true;

                let edit_event = EditEvent {
                    start_byte: byte_pos,
                    old_end_byte: byte_pos + 1,
                    new_end_byte: byte_pos,
                    start_position: (line, col_byte),
                    old_end_position: (line + 1, 0),
                    new_end_position: (line, col_byte),
                };
                self.pending_edits.push(edit_event.clone());

                self.history.record(
                    EditRecord {
                        char_offset: self.cursors[0],
                        old_text: "\n".to_string(),
                        new_text: String::new(),
                        edit_event,
                    },
                    &cursors_before,
                    &self.cursors,
                );
            }
        } else {
            // Delete from cursor to end of line
            let deleted: String = self.rope.slice(self.cursors[0]..line_end).to_string();
            let start_byte = self.rope.char_to_byte(self.cursors[0]);
            let end_byte = self.rope.char_to_byte(line_end);
            let line_byte_start = self.rope.line_to_byte(line);
            let start_col_byte = start_byte - line_byte_start;
            let end_col_byte = end_byte - line_byte_start;

            self.rope.remove(self.cursors[0]..line_end);
            self.dirty = true;

            let edit_event = EditEvent {
                start_byte,
                old_end_byte: end_byte,
                new_end_byte: start_byte,
                start_position: (line, start_col_byte),
                old_end_position: (line, end_col_byte),
                new_end_position: (line, start_col_byte),
            };
            self.pending_edits.push(edit_event.clone());

            self.history.record(
                EditRecord {
                    char_offset: self.cursors[0],
                    old_text: deleted,
                    new_text: String::new(),
                    edit_event,
                },
                &cursors_before,
                &self.cursors,
            );
        }
    }

    // -------------------------------------------------------
    // Transaction delegation
    // -------------------------------------------------------

    pub fn begin_transaction(&mut self) {
        self.history.begin_transaction(&self.cursors);
    }

    pub fn commit_transaction(&mut self) {
        self.history.commit_transaction();
    }

    pub fn flush_transaction(&mut self) {
        self.history.flush_transaction();
    }

    // -------------------------------------------------------
    // Undo / Redo
    // -------------------------------------------------------

    /// Undo the most recent edit transaction. Returns true if something was undone.
    pub fn undo(&mut self) -> bool {
        self.history.flush_transaction();
        let tx = match self.history.pop_undo() {
            Some(tx) => tx,
            None => return false,
        };

        // Apply records in reverse order
        for record in tx.records.iter().rev() {
            if !record.new_text.is_empty() {
                // This was an insertion — remove it
                let char_count = record.new_text.chars().count();
                self.rope
                    .remove(record.char_offset..record.char_offset + char_count);
            }
            if !record.old_text.is_empty() {
                // This was a deletion — re-insert the old text
                self.rope.insert(record.char_offset, &record.old_text);
            }

            // Generate EditEvent for tree-sitter (reversed direction)
            let byte_pos = self.rope.char_to_byte(record.char_offset);
            let line = self.rope.char_to_line(record.char_offset);
            let line_byte_start = self.rope.line_to_byte(line);
            let col_byte = byte_pos - line_byte_start;

            let old_text_bytes = record.new_text.len(); // what was new is now old
            let new_text_bytes = record.old_text.len(); // what was old is now new

            // Compute end positions for old (what was inserted, now being removed)
            let old_end_pos = self.compute_end_position(line, col_byte, &record.new_text);
            // Compute end positions for new (what was deleted, now being re-inserted)
            let new_end_pos = self.compute_end_position(line, col_byte, &record.old_text);

            self.pending_edits.push(EditEvent {
                start_byte: byte_pos,
                old_end_byte: byte_pos + old_text_bytes,
                new_end_byte: byte_pos + new_text_bytes,
                start_position: (line, col_byte),
                old_end_position: old_end_pos,
                new_end_position: new_end_pos,
            });
        }

        // Undo restores all cursor positions from before the edit
        self.cursors = tx.cursors_before.clone();
        self.sync_selection_head();
        self.dirty = true;

        // Push to redo stack
        self.history.push_redo(tx);
        true
    }

    /// Redo the most recently undone transaction. Returns true if something was redone.
    pub fn redo(&mut self) -> bool {
        self.history.flush_transaction();
        let tx = match self.history.pop_redo() {
            Some(tx) => tx,
            None => return false,
        };

        // Re-apply records in forward order
        for record in tx.records.iter() {
            if !record.old_text.is_empty() {
                // Remove the old text that was re-inserted by undo
                let char_count = record.old_text.chars().count();
                self.rope
                    .remove(record.char_offset..record.char_offset + char_count);
            }
            if !record.new_text.is_empty() {
                // Re-insert the new text
                self.rope.insert(record.char_offset, &record.new_text);
            }

            // Generate EditEvent for tree-sitter
            let byte_pos = self.rope.char_to_byte(record.char_offset);
            let line = self.rope.char_to_line(record.char_offset);
            let line_byte_start = self.rope.line_to_byte(line);
            let col_byte = byte_pos - line_byte_start;

            let old_text_bytes = record.old_text.len();
            let new_text_bytes = record.new_text.len();

            let old_end_pos = self.compute_end_position(line, col_byte, &record.old_text);
            let new_end_pos = self.compute_end_position(line, col_byte, &record.new_text);

            self.pending_edits.push(EditEvent {
                start_byte: byte_pos,
                old_end_byte: byte_pos + old_text_bytes,
                new_end_byte: byte_pos + new_text_bytes,
                start_position: (line, col_byte),
                old_end_position: old_end_pos,
                new_end_position: new_end_pos,
            });
        }

        // Redo restores all cursor positions from after the edit
        self.cursors = tx.cursors_after.clone();
        self.sync_selection_head();
        self.dirty = true;

        // Push back to undo stack
        self.history.push_undo(tx);
        true
    }

    /// Compute (row, col_byte) end position after applying text starting at (start_line, start_col_byte).
    fn compute_end_position(
        &self,
        start_line: usize,
        start_col_byte: usize,
        text: &str,
    ) -> (usize, usize) {
        if text.is_empty() {
            return (start_line, start_col_byte);
        }
        let newline_count = text.chars().filter(|&c| c == '\n').count();
        if newline_count == 0 {
            (start_line, start_col_byte + text.len())
        } else {
            let last_newline = text.rfind('\n').unwrap();
            let after_last_newline = &text[last_newline + 1..];
            (start_line + newline_count, after_last_newline.len())
        }
    }

    // -------------------------------------------------------
    // Selection
    // -------------------------------------------------------

    fn set_anchor_with_display(&mut self, cursor_display: SelectionCursorDisplay) {
        self.selection = Some(Selection {
            anchor: self.cursors[0],
            head: self.cursors[0],
            cursor_display,
        });
    }

    pub fn set_anchor(&mut self) {
        self.set_anchor_with_display(SelectionCursorDisplay::TailOnForward);
    }

    pub fn set_anchor_for_shift_extend(&mut self) {
        self.set_anchor_with_display(SelectionCursorDisplay::Head);
    }

    pub fn clear_anchor(&mut self) {
        if let Some(selection) = self.selection
            && matches!(
                selection.cursor_display,
                SelectionCursorDisplay::TailOnForward
            )
            && selection.head > selection.anchor
        {
            // Forward selection: all cursors are "one past" their display
            // position (exclusive end).  Adjust every cursor back by one so
            // that subsequent motions start from the displayed position.
            for cursor in &mut self.cursors {
                *cursor = cursor.saturating_sub(1);
            }
        }
        self.selection = None;
        self.sort_and_dedup_cursors();
    }

    /// Returns the half-open selection range `[start, end)`.
    pub fn selection_range(&self) -> Option<(usize, usize)> {
        let selection = self.selection?;
        let start = selection.anchor.min(selection.head);
        let end = selection
            .anchor
            .max(selection.head)
            .min(self.rope.len_chars());
        Some((start, end))
    }

    pub fn selection_text(&self) -> Option<String> {
        let (start, end) = self.selection_range()?;
        Some(self.rope.slice(start..end).to_string())
    }

    /// Select the current line as a linewise span:
    /// includes trailing newline when present.
    pub fn select_line(&mut self) {
        let line = self.cursor_line();
        let line_start = self.rope.line_to_char(line);
        let line_end = if line + 1 < self.rope.len_lines() {
            self.rope.line_to_char(line + 1)
        } else {
            self.rope.len_chars()
        };
        let head = line_end;
        self.selection = Some(Selection::tail_on_forward(line_start, head));
        // Head is one-past-the-end; display cursor shows the last selected char.
        self.cursors[0] = head;
    }

    /// Extend line selection down by one line. Keeps anchor, moves cursor to
    /// end of next line.
    pub fn extend_line_selection_down(&mut self) {
        let line = self.display_cursor_line();
        if line + 1 < self.rope.len_lines() {
            let next_line = line + 1;
            let next_end = if next_line + 1 < self.rope.len_lines() {
                self.rope.line_to_char(next_line + 1)
            } else {
                self.rope.len_chars()
            };
            self.cursors[0] = next_end;
            self.sync_selection_head();
        }
    }

    /// Delete a char range `[start, end)` with full undo/redo recording.
    /// Returns the deleted text.
    pub fn delete_range(&mut self, start: usize, end: usize) -> String {
        if start >= end || start >= self.rope.len_chars() {
            return String::new();
        }
        let end = end.min(self.rope.len_chars());
        let cursors_before = self.cursors.clone();
        let deleted: String = self.rope.slice(start..end).to_string();

        let start_byte = self.rope.char_to_byte(start);
        let end_byte = self.rope.char_to_byte(end);
        let start_line = self.rope.char_to_line(start);
        let start_line_byte = self.rope.line_to_byte(start_line);
        let start_col_byte = start_byte - start_line_byte;

        let end_line = self.rope.char_to_line(end);
        let end_line_byte = self.rope.line_to_byte(end_line);
        let end_col_byte = end_byte - end_line_byte;

        self.rope.remove(start..end);
        self.dirty = true;

        // Place cursor at start of deleted range (single cursor after delete_range)
        self.cursors = vec![start.min(self.rope.len_chars())];
        self.sync_selection_head();

        let edit_event = EditEvent {
            start_byte,
            old_end_byte: end_byte,
            new_end_byte: start_byte,
            start_position: (start_line, start_col_byte),
            old_end_position: (end_line, end_col_byte),
            new_end_position: (start_line, start_col_byte),
        };
        self.pending_edits.push(edit_event.clone());

        self.history.record(
            EditRecord {
                char_offset: start,
                old_text: deleted.clone(),
                new_text: String::new(),
                edit_event,
            },
            &cursors_before,
            &self.cursors,
        );

        deleted
    }

    /// Insert text at a given char position with full undo/redo recording.
    pub fn insert_text_at(&mut self, pos: usize, text: &str) {
        if text.is_empty() {
            return;
        }
        let text = Self::normalize_newlines_for_insert(text);
        let text = text.as_ref();

        let cursors_before = self.cursors.clone();
        let pos = pos.min(self.rope.len_chars());

        let byte_pos = self.rope.char_to_byte(pos);
        let line = self.rope.char_to_line(pos);
        let line_byte_start = self.rope.line_to_byte(line);
        let col_byte = byte_pos - line_byte_start;

        self.rope.insert(pos, text);
        let char_count = text.chars().count();
        self.cursors[0] = pos + char_count;
        self.sync_selection_head();
        self.dirty = true;

        let new_end_pos = self.compute_end_position(line, col_byte, text);

        let edit_event = EditEvent {
            start_byte: byte_pos,
            old_end_byte: byte_pos,
            new_end_byte: byte_pos + text.len(),
            start_position: (line, col_byte),
            old_end_position: (line, col_byte),
            new_end_position: new_end_pos,
        };
        self.pending_edits.push(edit_event.clone());

        self.history.record(
            EditRecord {
                char_offset: pos,
                old_text: String::new(),
                new_text: text.to_string(),
                edit_event,
            },
            &cursors_before,
            &self.cursors,
        );
    }

    /// Length of line content excluding trailing newline
    fn line_len(&self, line_idx: usize) -> usize {
        let line = self.rope.line(line_idx);
        let len = line.len_chars();
        if len > 0 && line.char(len - 1) == '\n' {
            len - 1
        } else {
            len
        }
    }

    fn line_display_width(&self, line_idx: usize) -> usize {
        let line = self.rope.line(line_idx);
        let mut width = 0usize;
        for idx in 0..line.len_chars() {
            let ch = line.char(idx);
            if ch == '\n' {
                break;
            }
            width += char_display_width(ch);
        }
        width
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MARKDOWN_JA: &str = "\
# 竹取物語

## あらすじ

今は昔、竹取の翁といふものありけり。
野山にまじりて竹を取りつつ、よろづのことに使ひけり。

## 登場人物

- **かぐや姫** — 竹の中から見つかった少女
- **翁（おきな）** — 竹取の翁
- **媼（おうな）** — 翁の妻

## コードブロック

```rust
fn main() {
    println!(\"かぐや\");
}
```

> 天人の中に持たせたる箱あり。天の羽衣入れり。

以上。
";

    fn doc_ja() -> Document {
        let mut doc = Document::new_scratch(1);
        doc.rope = Rope::from_str(MARKDOWN_JA);
        doc
    }

    fn doc_from_str(s: &str) -> Document {
        let mut doc = Document::new_scratch(1);
        doc.rope = Rope::from_str(s);
        doc
    }

    // -------------------------------------------------------
    // Line / char structure
    // -------------------------------------------------------

    #[test]
    fn line_count() {
        let doc = doc_ja();
        let expected =
            MARKDOWN_JA.lines().count() + if MARKDOWN_JA.ends_with('\n') { 1 } else { 0 };
        assert_eq!(doc.rope.len_lines(), expected);
    }

    #[test]
    fn char_count_matches() {
        let doc = doc_ja();
        assert_eq!(doc.rope.len_chars(), MARKDOWN_JA.chars().count());
    }

    #[test]
    fn first_line_is_heading() {
        let doc = doc_ja();
        let line = doc.rope.line(0).to_string();
        assert_eq!(line.trim_end_matches('\n'), "# 竹取物語");
    }

    #[test]
    fn line_with_bold_markdown() {
        let doc = doc_ja();
        let idx = (0..doc.rope.len_lines())
            .find(|&i| doc.rope.line(i).to_string().contains("**かぐや姫**"))
            .expect("bold markdown line not found");
        let line = doc.rope.line(idx).to_string();
        assert!(line.starts_with("- "));
    }

    #[test]
    fn code_block_fence() {
        let doc = doc_ja();
        let idx = (0..doc.rope.len_lines())
            .find(|&i| doc.rope.line(i).to_string().starts_with("```rust"))
            .expect("code fence not found");
        let next = doc.rope.line(idx + 1).to_string();
        assert!(next.contains("fn main"));
    }

    #[test]
    fn blockquote_line() {
        let doc = doc_ja();
        let idx = (0..doc.rope.len_lines())
            .find(|&i| doc.rope.line(i).to_string().starts_with("> "))
            .expect("blockquote not found");
        let line = doc.rope.line(idx).to_string();
        assert!(line.contains("天の羽衣"));
    }

    // -------------------------------------------------------
    // Cursor movement over multi-byte chars
    // -------------------------------------------------------

    #[test]
    fn move_right_across_japanese() {
        let mut doc = doc_ja();
        assert_eq!(doc.cursors[0], 0);
        doc.move_right(); // '#'
        doc.move_right(); // ' '
        assert_eq!(doc.cursors[0], 2);
        assert_eq!(doc.rope.char(doc.cursors[0]), '竹');
        doc.move_right(); // '竹'
        assert_eq!(doc.rope.char(doc.cursors[0]), '取');
    }

    #[test]
    fn move_left_across_japanese() {
        let mut doc = doc_ja();
        doc.cursors[0] = 4;
        assert_eq!(doc.rope.char(doc.cursors[0]), '物');
        doc.move_left();
        assert_eq!(doc.rope.char(doc.cursors[0]), '取');
        doc.move_left();
        assert_eq!(doc.rope.char(doc.cursors[0]), '竹');
    }

    #[test]
    fn move_right_stops_at_end() {
        let mut doc = doc_from_str("あ");
        doc.cursors[0] = 1;
        doc.move_right();
        assert_eq!(doc.cursors[0], 1);
    }

    #[test]
    fn move_left_stops_at_zero() {
        let mut doc = doc_ja();
        doc.move_left();
        assert_eq!(doc.cursors[0], 0);
    }

    #[test]
    fn move_down_clamps_col_to_shorter_line() {
        let mut doc = doc_from_str("あいうえお\nかき\n");
        doc.move_to_line_end();
        assert_eq!(doc.cursor_col(), 5);
        doc.move_down();
        assert_eq!(doc.cursor_line(), 1);
        assert_eq!(doc.cursor_col(), 2);
    }

    #[test]
    fn move_up_clamps_col() {
        let mut doc = doc_from_str("あ\nかきくけこ\n");
        doc.cursors[0] = doc.rope.line_to_char(1) + 5;
        assert_eq!(doc.cursor_col(), 5);
        doc.move_up();
        assert_eq!(doc.cursor_line(), 0);
        assert_eq!(doc.cursor_col(), 1);
    }

    #[test]
    fn line_start_and_end() {
        let mut doc = doc_ja();
        doc.cursors[0] = 3;
        doc.move_to_line_start();
        assert_eq!(doc.cursors[0], 0);
        doc.move_to_line_end();
        assert_eq!(doc.cursor_col(), 6);
    }

    // -------------------------------------------------------
    // Editing with Japanese text
    // -------------------------------------------------------

    #[test]
    fn insert_japanese_char() {
        let mut doc = doc_from_str("あいう\n");
        doc.cursors[0] = 1;
        doc.insert_char('ん');
        assert_eq!(doc.rope.line(0).to_string(), "あんいう\n");
        assert_eq!(doc.cursors[0], 2);
        assert!(doc.dirty);
    }

    #[test]
    fn insert_newline_splits_japanese_line() {
        let mut doc = doc_from_str("あいう\n");
        doc.cursors[0] = 2;
        doc.insert_newline();
        assert_eq!(doc.rope.line(0).to_string(), "あい\n");
        assert_eq!(doc.rope.line(1).to_string(), "う\n");
        assert_eq!(doc.cursor_line(), 1);
        assert_eq!(doc.cursor_col(), 0);
    }

    #[test]
    fn delete_forward_japanese() {
        let mut doc = doc_from_str("かきく\n");
        doc.cursors[0] = 1;
        doc.delete_forward();
        assert_eq!(doc.rope.line(0).to_string(), "かく\n");
        assert_eq!(doc.cursors[0], 1);
    }

    #[test]
    fn delete_backward_japanese() {
        let mut doc = doc_from_str("かきく\n");
        doc.cursors[0] = 2;
        doc.delete_backward();
        assert_eq!(doc.rope.line(0).to_string(), "かく\n");
        assert_eq!(doc.cursors[0], 1);
    }

    #[test]
    fn kill_line_japanese() {
        let mut doc = doc_from_str("あいうえお\nかきく\n");
        doc.cursors[0] = 2;
        doc.kill_line();
        assert_eq!(doc.rope.line(0).to_string(), "あい\n");
        assert_eq!(doc.cursors[0], 2);
    }

    #[test]
    fn kill_line_at_eol_joins_lines() {
        let mut doc = doc_from_str("あ\nい\n");
        doc.cursors[0] = 1;
        doc.kill_line();
        assert_eq!(doc.rope.to_string(), "あい\n");
    }

    #[test]
    fn kill_line_on_markdown_heading() {
        let mut doc = doc_ja();
        doc.cursors[0] = 2;
        doc.kill_line();
        assert_eq!(doc.rope.line(0).to_string(), "# \n");
    }

    // -------------------------------------------------------
    // Scroll
    // -------------------------------------------------------

    #[test]
    fn ensure_cursor_visible_scrolls_down() {
        let mut doc = doc_ja();
        let last_line = doc.rope.len_lines() - 1;
        doc.cursors[0] = doc.rope.line_to_char(last_line);
        doc.ensure_cursor_visible(5);
        assert!(doc.scroll_offset > 0);
        assert!(doc.cursor_line() < doc.scroll_offset + 5);
    }

    #[test]
    fn ensure_cursor_visible_scrolls_up() {
        let mut doc = doc_ja();
        doc.scroll_offset = 10;
        doc.cursors[0] = 0;
        doc.ensure_cursor_visible(5);
        assert_eq!(doc.scroll_offset, 0);
    }

    #[test]
    fn ensure_cursor_visible_with_horizontal_scrolls_right_after_margin() {
        let mut doc = doc_from_str("0123456789abcdefghijklmnopqrstuvwxyz\n");
        let view_width = 10;
        let margin = 2;

        // right trigger = 10 - 1 - 2 = 7. At col 7 it should not scroll yet.
        doc.set_cursor_line_char(0, 7);
        doc.ensure_cursor_visible_with_horizontal(5, view_width, margin);
        assert_eq!(doc.horizontal_scroll_offset, 0);

        // Crossing trigger should scroll.
        doc.set_cursor_line_char(0, 8);
        doc.ensure_cursor_visible_with_horizontal(5, view_width, margin);
        assert_eq!(doc.horizontal_scroll_offset, 1);
    }

    #[test]
    fn ensure_cursor_visible_with_horizontal_scrolls_left_after_margin() {
        let mut doc = doc_from_str("0123456789abcdefghijklmnopqrstuvwxyz\n");
        let view_width = 10;
        let margin = 2;
        doc.horizontal_scroll_offset = 10;

        // left trigger = 10 + 2 = 12. At col 12 it should not scroll.
        doc.set_cursor_line_char(0, 12);
        doc.ensure_cursor_visible_with_horizontal(5, view_width, margin);
        assert_eq!(doc.horizontal_scroll_offset, 10);

        // Crossing left trigger should scroll back.
        doc.set_cursor_line_char(0, 11);
        doc.ensure_cursor_visible_with_horizontal(5, view_width, margin);
        assert_eq!(doc.horizontal_scroll_offset, 9);
    }

    #[test]
    fn ensure_cursor_visible_with_horizontal_resets_when_line_fits() {
        let mut doc = doc_from_str("short\nvery very long line here\n");
        doc.horizontal_scroll_offset = 7;
        doc.set_cursor_line_char(0, 2);
        doc.ensure_cursor_visible_with_horizontal(5, 20, 5);
        assert_eq!(doc.horizontal_scroll_offset, 0);
    }

    #[test]
    fn display_cursor_display_col_counts_tabs() {
        let mut doc = doc_from_str("\tb\n");
        doc.set_cursor_line_char(0, 1);
        assert_eq!(doc.display_cursor_display_col(), 4);
    }

    // -------------------------------------------------------
    // scroll_viewport
    // -------------------------------------------------------

    fn ten_line_doc() -> Document {
        let content: String = (0..10).map(|i| format!("line {i}\n")).collect();
        doc_from_str(&content)
    }

    #[test]
    fn scroll_viewport_down() {
        let mut doc = ten_line_doc();
        doc.scroll_offset = 0;
        doc.cursors[0] = 0;
        doc.scroll_viewport(3, 5);
        assert_eq!(doc.scroll_offset, 3);
        // Cursor was at line 0, now outside viewport (0 < 3), so it should move to line 3
        assert_eq!(doc.cursor_line(), 3);
    }

    #[test]
    fn scroll_viewport_up() {
        let mut doc = ten_line_doc();
        doc.scroll_offset = 5;
        doc.set_cursor_line_char(5, 0);
        doc.scroll_viewport(-3, 5);
        assert_eq!(doc.scroll_offset, 2);
        // Cursor at line 5 is within viewport [2..7), stays put
        assert_eq!(doc.cursor_line(), 5);
    }

    #[test]
    fn scroll_viewport_clamps_at_zero() {
        let mut doc = ten_line_doc();
        doc.scroll_offset = 1;
        doc.cursors[0] = doc.rope.line_to_char(1);
        doc.scroll_viewport(-10, 5);
        assert_eq!(doc.scroll_offset, 0);
    }

    #[test]
    fn scroll_viewport_clamps_at_end() {
        let mut doc = ten_line_doc();
        doc.scroll_viewport(100, 5);
        // 10 content lines + 1 trailing empty line = 11 lines, max scroll = 10
        assert_eq!(doc.scroll_offset, doc.rope.len_lines() - 1);
    }

    #[test]
    fn scroll_viewport_preserves_column() {
        let mut doc =
            doc_from_str("abcdef\nghijkl\nmnopqr\nstuvwx\nyz1234\n56789a\nbcdefg\nhijklm\n");
        doc.set_cursor_line_char(0, 3); // cursor at col 3 of line 0
        doc.scroll_viewport(3, 3);
        // Cursor should have moved to line 3, col 3
        assert_eq!(doc.cursor_line(), 3);
        let line_start = doc.rope.line_to_char(3);
        assert_eq!(doc.cursors[0] - line_start, 3);
    }

    #[test]
    fn scroll_viewport_no_op_when_cursor_in_view() {
        let mut doc = ten_line_doc();
        doc.scroll_offset = 0;
        doc.set_cursor_line_char(2, 0);
        let cursor_before = doc.cursors[0];
        doc.scroll_viewport(1, 5);
        assert_eq!(doc.scroll_offset, 1);
        // Cursor at line 2 is within [1..6), should not move
        assert_eq!(doc.cursors[0], cursor_before);
    }

    #[test]
    fn scroll_viewport_ensure_cursor_visible_is_noop() {
        let mut doc = ten_line_doc();
        doc.scroll_offset = 0;
        doc.set_cursor_line_char(2, 0);
        doc.scroll_viewport(3, 5);
        let scroll_after = doc.scroll_offset;
        let cursor_after = doc.cursors[0];
        // ensure_cursor_visible should be a no-op since cursor is already in viewport
        doc.ensure_cursor_visible(5);
        assert_eq!(doc.scroll_offset, scroll_after);
        assert_eq!(doc.cursors[0], cursor_after);
    }

    // -------------------------------------------------------
    // Word motions
    // -------------------------------------------------------

    #[test]
    fn word_forward_ascii() {
        let mut doc = doc_from_str("hello world foo");
        doc.move_word_forward();
        assert_eq!(doc.cursors[0], 6);
        doc.move_word_forward();
        assert_eq!(doc.cursors[0], 12);
    }

    #[test]
    fn word_forward_with_punctuation() {
        let mut doc = doc_from_str("foo, bar");
        doc.move_word_forward();
        assert_eq!(doc.cursors[0], 3);
        doc.move_word_forward();
        assert_eq!(doc.cursors[0], 5);
    }

    #[test]
    fn word_forward_end_ascii() {
        let mut doc = doc_from_str("hello world");
        doc.move_word_forward_end();
        assert_eq!(doc.cursors[0], 4);
        doc.move_word_forward_end();
        assert_eq!(doc.cursors[0], 10);
    }

    #[test]
    fn word_backward_ascii() {
        let mut doc = doc_from_str("hello world foo");
        doc.cursors[0] = 12;
        doc.move_word_backward();
        assert_eq!(doc.cursors[0], 6);
        doc.move_word_backward();
        assert_eq!(doc.cursors[0], 0);
    }

    #[test]
    fn word_forward_japanese() {
        let mut doc = doc_from_str("hello 世界 test");
        doc.move_word_forward();
        assert_eq!(doc.cursors[0], 6);
    }

    #[test]
    fn word_backward_stops_at_zero() {
        let mut doc = doc_from_str("hello");
        doc.cursors[0] = 0;
        doc.move_word_backward();
        assert_eq!(doc.cursors[0], 0);
    }

    #[test]
    fn word_forward_stops_at_end() {
        let mut doc = doc_from_str("hello");
        doc.cursors[0] = 5;
        doc.move_word_forward();
        assert_eq!(doc.cursors[0], 5);
    }

    #[test]
    fn long_word_forward_treats_punctuation_as_same_word() {
        let mut doc = doc_from_str("foo.bar baz");
        doc.move_long_word_forward();
        assert_eq!(doc.cursors[0], 8);
    }

    #[test]
    fn long_word_backward_treats_punctuation_as_same_word() {
        let mut doc = doc_from_str("foo.bar baz");
        doc.cursors[0] = 8;
        doc.move_long_word_backward();
        assert_eq!(doc.cursors[0], 0);
    }

    #[test]
    fn visual_extend_word_keeps_anchor() {
        let mut doc = doc_from_str("hello world");
        doc.cursors[0] = 0;
        doc.set_anchor();
        doc.extend_word_forward();
        assert_eq!(doc.selection_anchor(), Some(0));
        assert_eq!(doc.cursors[0], 6);
    }

    #[test]
    fn visual_extend_word_backward_keeps_anchor() {
        let mut doc = doc_from_str("hello world");
        doc.cursors[0] = 6;
        doc.set_anchor();
        doc.extend_word_backward();
        assert_eq!(doc.selection_anchor(), Some(6));
        assert_eq!(doc.cursors[0], 0);
    }

    // -------------------------------------------------------
    // File I/O round-trip with Japanese markdown
    // -------------------------------------------------------

    #[test]
    fn save_and_reopen_japanese() {
        let dir = std::env::temp_dir().join("kaguya_test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("test_ja_doc.md");
        let path_str = path.to_str().unwrap();

        let mut doc = doc_from_str(MARKDOWN_JA);
        doc.file_path = Some(path.clone());
        doc.dirty = true;
        doc.save().unwrap();
        assert!(!doc.dirty);

        let doc2 = Document::from_file(2, path_str);
        assert_eq!(doc2.rope.to_string(), MARKDOWN_JA);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn save_as_sets_file_path_and_clears_dirty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("saved.md");

        let mut doc = doc_from_str("hello");
        doc.dirty = true;
        let msg = doc.save_as(&path).unwrap();
        let canonical = fs::canonicalize(&path).unwrap_or_else(|_| path.clone());

        assert_eq!(doc.file_path.as_deref(), Some(canonical.as_path()));
        assert!(!doc.dirty);
        assert!(msg.contains("Wrote"));
        assert_eq!(fs::read_to_string(path).unwrap(), "hello");
    }

    #[test]
    fn save_as_overwrites_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("overwrite.txt");
        fs::write(&path, "old").unwrap();

        let mut doc = doc_from_str("new");
        doc.save_as(&path).unwrap();

        assert_eq!(fs::read_to_string(path).unwrap(), "new");
    }

    #[test]
    fn save_as_creates_missing_parent_directories() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("deep").join("file.txt");

        let mut doc = doc_from_str("created");
        doc.save_as(&path).unwrap();

        assert!(path.exists());
        assert_eq!(fs::read_to_string(path).unwrap(), "created");
    }

    #[test]
    fn save_as_updates_status_bar_path_cache() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("status.rs");

        let mut doc = doc_from_str("fn main() {}");
        doc.save_as(&path).unwrap();

        assert_ne!(doc.status_bar_path(), "[scratch]");
    }

    // -------------------------------------------------------
    // display_name
    // -------------------------------------------------------

    #[test]
    fn display_name_scratch() {
        let doc = Document::new_scratch(1);
        assert_eq!(doc.display_name(), "[scratch]");
    }

    #[test]
    fn display_name_with_path() {
        let doc = Document::from_file(1, "src/main.rs");
        assert_eq!(doc.display_name(), "src/main.rs");
    }

    #[test]
    fn status_bar_path_scratch() {
        let doc = Document::new_scratch(1);
        assert_eq!(doc.status_bar_path(), "[scratch]");
    }

    #[test]
    fn status_bar_path_in_git_repo() {
        // Test with the current file (document.rs) which is in a git repo
        let doc = Document::from_file(1, "src/core/document.rs");
        let path = doc.status_bar_path();
        // Should be in format "[repo_name] relative/path"
        assert!(path.starts_with('['));
        assert!(path.contains("] "));
        assert!(path.contains("src/core/document.rs"));
    }

    #[test]
    fn extract_repo_name_from_github_ssh() {
        let remote = "git@github.com:user/my-repo.git";
        let name = Document::extract_repo_name_from_remote(remote);
        assert_eq!(name, Some("my-repo".to_string()));
    }

    #[test]
    fn extract_repo_name_from_github_https() {
        let remote = "https://github.com/user/my-repo.git";
        let name = Document::extract_repo_name_from_remote(remote);
        assert_eq!(name, Some("my-repo".to_string()));
    }

    #[test]
    fn extract_repo_name_without_dot_git() {
        let remote = "https://github.com/user/my-repo";
        let name = Document::extract_repo_name_from_remote(remote);
        assert_eq!(name, Some("my-repo".to_string()));
    }

    // -------------------------------------------------------
    // Undo / Redo
    // -------------------------------------------------------

    #[test]
    fn undo_insert_char() {
        let mut doc = doc_from_str("abc\n");
        doc.cursors[0] = 1;
        doc.insert_char('X');
        assert_eq!(doc.rope.to_string(), "aXbc\n");
        assert_eq!(doc.cursors[0], 2);

        assert!(doc.undo());
        assert_eq!(doc.rope.to_string(), "abc\n");
        assert_eq!(doc.cursors[0], 1);
    }

    #[test]
    fn redo_insert_char() {
        let mut doc = doc_from_str("abc\n");
        doc.cursors[0] = 1;
        doc.insert_char('X');
        doc.undo();
        assert_eq!(doc.rope.to_string(), "abc\n");

        assert!(doc.redo());
        assert_eq!(doc.rope.to_string(), "aXbc\n");
        assert_eq!(doc.cursors[0], 2);
    }

    #[test]
    fn undo_delete_backward() {
        let mut doc = doc_from_str("abc\n");
        doc.cursors[0] = 2;
        doc.delete_backward();
        assert_eq!(doc.rope.to_string(), "ac\n");
        assert_eq!(doc.cursors[0], 1);

        assert!(doc.undo());
        assert_eq!(doc.rope.to_string(), "abc\n");
        assert_eq!(doc.cursors[0], 2);
    }

    #[test]
    fn undo_delete_forward() {
        let mut doc = doc_from_str("abc\n");
        doc.cursors[0] = 1;
        doc.delete_forward();
        assert_eq!(doc.rope.to_string(), "ac\n");

        assert!(doc.undo());
        assert_eq!(doc.rope.to_string(), "abc\n");
        assert_eq!(doc.cursors[0], 1);
    }

    #[test]
    fn undo_kill_line() {
        let mut doc = doc_from_str("hello world\n");
        doc.cursors[0] = 5;
        doc.kill_line();
        assert_eq!(doc.rope.to_string(), "hello\n");

        assert!(doc.undo());
        assert_eq!(doc.rope.to_string(), "hello world\n");
        assert_eq!(doc.cursors[0], 5);
    }

    #[test]
    fn undo_insert_newline() {
        let mut doc = doc_from_str("abc\n");
        doc.cursors[0] = 2;
        doc.insert_newline();
        assert_eq!(doc.rope.to_string(), "ab\nc\n");

        assert!(doc.undo());
        assert_eq!(doc.rope.to_string(), "abc\n");
        assert_eq!(doc.cursors[0], 2);
    }

    #[test]
    fn undo_japanese_insert() {
        let mut doc = doc_from_str("あいう\n");
        doc.cursors[0] = 1;
        doc.insert_char('ん');
        assert_eq!(doc.rope.to_string(), "あんいう\n");

        assert!(doc.undo());
        assert_eq!(doc.rope.to_string(), "あいう\n");
        assert_eq!(doc.cursors[0], 1);
    }

    #[test]
    fn undo_japanese_kill_line() {
        let mut doc = doc_from_str("あいうえお\n");
        doc.cursors[0] = 2;
        doc.kill_line();
        assert_eq!(doc.rope.to_string(), "あい\n");

        assert!(doc.undo());
        assert_eq!(doc.rope.to_string(), "あいうえお\n");
        assert_eq!(doc.cursors[0], 2);
    }

    #[test]
    fn multiple_undo_redo() {
        let mut doc = doc_from_str("");
        doc.insert_char('a');
        doc.insert_char('b');
        doc.insert_char('c');
        assert_eq!(doc.rope.to_string(), "abc");

        doc.undo(); // remove 'c'
        assert_eq!(doc.rope.to_string(), "ab");
        doc.undo(); // remove 'b'
        assert_eq!(doc.rope.to_string(), "a");
        doc.redo(); // re-insert 'b'
        assert_eq!(doc.rope.to_string(), "ab");
        doc.redo(); // re-insert 'c'
        assert_eq!(doc.rope.to_string(), "abc");
    }

    #[test]
    fn undo_nothing_returns_false() {
        let mut doc = doc_from_str("hello");
        assert!(!doc.undo());
    }

    #[test]
    fn redo_nothing_returns_false() {
        let mut doc = doc_from_str("hello");
        assert!(!doc.redo());
    }

    #[test]
    fn new_edit_clears_redo() {
        let mut doc = doc_from_str("");
        doc.insert_char('a');
        doc.insert_char('b');
        doc.undo(); // redo has 'b'
        doc.insert_char('c'); // should clear redo
        assert!(!doc.redo()); // redo stack cleared
        assert_eq!(doc.rope.to_string(), "ac");
    }

    // -------------------------------------------------------
    // Multi-cursor undo/redo
    // -------------------------------------------------------

    #[test]
    fn multi_cursor_undo_insert_char() {
        let mut doc = doc_from_str("ab\ncd\n");
        doc.cursors[0] = 0; // before 'a'
        doc.cursors.push(3); // before 'c'
        doc.sort_and_dedup_cursors();
        assert_eq!(doc.cursor_count(), 2);

        doc.insert_char('X');
        assert_eq!(doc.rope.to_string(), "Xab\nXcd\n");

        // Undo should reverse BOTH insertions
        assert!(doc.undo());
        assert_eq!(doc.rope.to_string(), "ab\ncd\n");
        // All cursor positions should be restored
        assert_eq!(doc.cursor_count(), 2);
        assert_eq!(doc.cursors[0], 0);
        assert_eq!(doc.cursors[1], 3);
    }

    #[test]
    fn multi_cursor_undo_delete_backward() {
        let mut doc = doc_from_str("ab\ncd\n");
        doc.cursors[0] = 1; // after 'a'
        doc.cursors.push(4); // after 'c'
        doc.sort_and_dedup_cursors();
        assert_eq!(doc.cursor_count(), 2);

        doc.delete_backward();
        assert_eq!(doc.rope.to_string(), "b\nd\n");

        // Undo should restore BOTH deleted characters
        assert!(doc.undo());
        assert_eq!(doc.rope.to_string(), "ab\ncd\n");
        // All cursor positions should be restored
        assert_eq!(doc.cursor_count(), 2);
        assert_eq!(doc.cursors[0], 1);
        assert_eq!(doc.cursors[1], 4);
    }

    #[test]
    fn multi_cursor_redo_insert_char() {
        let mut doc = doc_from_str("ab\ncd\n");
        doc.cursors[0] = 0;
        doc.cursors.push(3);
        doc.sort_and_dedup_cursors();

        doc.insert_char('X');
        assert_eq!(doc.rope.to_string(), "Xab\nXcd\n");

        doc.undo();
        assert_eq!(doc.rope.to_string(), "ab\ncd\n");

        // Redo should re-insert BOTH characters
        assert!(doc.redo());
        assert_eq!(doc.rope.to_string(), "Xab\nXcd\n");
        // Cursor positions should be restored to after insertions
        assert_eq!(doc.cursor_count(), 2);
        assert_eq!(doc.cursors[0], 1);
        assert_eq!(doc.cursors[1], 5);
    }

    // -------------------------------------------------------
    // Transaction-based undo/redo
    // -------------------------------------------------------

    #[test]
    fn grouped_insert_single_undo() {
        let mut doc = doc_from_str("");
        doc.begin_transaction();
        doc.insert_char('a');
        doc.insert_char('b');
        doc.insert_char('c');
        doc.commit_transaction();
        assert_eq!(doc.rope.to_string(), "abc");

        assert!(doc.undo());
        assert_eq!(doc.rope.to_string(), "");
        assert_eq!(doc.cursors[0], 0);
    }

    #[test]
    fn grouped_insert_undo_redo_round_trip() {
        let mut doc = doc_from_str("");
        doc.begin_transaction();
        doc.insert_char('a');
        doc.insert_char('b');
        doc.insert_char('c');
        doc.commit_transaction();

        assert!(doc.undo());
        assert_eq!(doc.rope.to_string(), "");

        assert!(doc.redo());
        assert_eq!(doc.rope.to_string(), "abc");
        assert_eq!(doc.cursors[0], 3);
    }

    #[test]
    fn mixed_normal_and_insert_undo() {
        let mut doc = doc_from_str("hello\n");
        // Normal mode: atomic delete
        doc.cursors[0] = 0;
        doc.delete_forward(); // delete 'h'
        assert_eq!(doc.rope.to_string(), "ello\n");

        // Insert mode: grouped inserts
        doc.begin_transaction();
        doc.insert_char('H');
        doc.insert_char('i');
        doc.commit_transaction();
        assert_eq!(doc.rope.to_string(), "Hiello\n");

        // Undo grouped insert
        assert!(doc.undo());
        assert_eq!(doc.rope.to_string(), "ello\n");

        // Undo atomic delete
        assert!(doc.undo());
        assert_eq!(doc.rope.to_string(), "hello\n");
    }

    #[test]
    fn undo_flushes_open_transaction() {
        let mut doc = doc_from_str("");
        doc.begin_transaction();
        doc.insert_char('a');
        doc.insert_char('b');
        // undo without explicit commit — should flush first
        assert!(doc.undo());
        assert_eq!(doc.rope.to_string(), "");
    }

    #[test]
    fn empty_insert_session_no_undo_entry() {
        let mut doc = doc_from_str("hello");
        doc.begin_transaction();
        // No edits
        doc.commit_transaction();
        // Nothing to undo
        assert!(!doc.undo());
    }

    #[test]
    fn grouped_insert_with_backspace() {
        let mut doc = doc_from_str("");
        doc.begin_transaction();
        doc.insert_char('a');
        doc.insert_char('b');
        doc.insert_char('c');
        doc.delete_backward(); // delete 'c'
        doc.commit_transaction();
        assert_eq!(doc.rope.to_string(), "ab");

        assert!(doc.undo());
        assert_eq!(doc.rope.to_string(), "");
        assert_eq!(doc.cursors[0], 0);
    }

    #[test]
    fn grouped_insert_with_newline() {
        let mut doc = doc_from_str("");
        doc.begin_transaction();
        doc.insert_char('a');
        doc.insert_newline();
        doc.insert_char('b');
        doc.commit_transaction();
        assert_eq!(doc.rope.to_string(), "a\nb");

        assert!(doc.undo());
        assert_eq!(doc.rope.to_string(), "");
        assert_eq!(doc.cursors[0], 0);
    }

    // -------------------------------------------------------
    // OpenLineBelow (o)
    //
    // Simulates the `o` command sequence at Document level:
    //   move_to_line_end → begin_transaction → insert_newline
    // -------------------------------------------------------

    /// Helper: perform the `o` (open-line-below) sequence on a document.
    fn open_line_below(doc: &mut Document) {
        doc.move_to_line_end();
        doc.begin_transaction();
        doc.insert_newline();
    }

    #[test]
    fn open_line_below_basic() {
        let mut doc = doc_from_str("hello\nworld\n");
        // Cursor in the middle of line 0
        doc.cursors[0] = 2;
        open_line_below(&mut doc);
        doc.commit_transaction();

        // A newline was inserted at the end of "hello", producing "hello\n\nworld\n"
        assert_eq!(doc.rope.to_string(), "hello\n\nworld\n");
        // Cursor is on the new (empty) line 1, column 0
        assert_eq!(doc.cursor_line(), 1);
        assert_eq!(doc.cursor_col(), 0);
    }

    #[test]
    fn open_line_below_last_line() {
        let mut doc = doc_from_str("alpha\nbeta\n");
        // Place cursor on the last logical line (the empty line after trailing '\n')
        let last_line = doc.rope.len_lines() - 1;
        doc.cursors[0] = doc.rope.line_to_char(last_line);
        open_line_below(&mut doc);
        doc.commit_transaction();

        // A new line is appended at the very end
        assert_eq!(doc.rope.to_string(), "alpha\nbeta\n\n");
        assert_eq!(doc.cursor_line(), last_line + 1);
        assert_eq!(doc.cursor_col(), 0);
    }

    #[test]
    fn open_line_below_empty_doc() {
        let mut doc = doc_from_str("");
        open_line_below(&mut doc);
        doc.commit_transaction();

        assert_eq!(doc.rope.to_string(), "\n");
        assert_eq!(doc.cursor_line(), 1);
        assert_eq!(doc.cursor_col(), 0);
    }

    #[test]
    fn open_line_below_japanese() {
        let mut doc = doc_from_str("あいうえお\nかきくけこ\n");
        // Cursor in the middle of the first Japanese line
        doc.cursors[0] = 2; // after 'あ','い'
        open_line_below(&mut doc);
        doc.commit_transaction();

        // Original content preserved; new line inserted after line 0
        assert_eq!(doc.rope.to_string(), "あいうえお\n\nかきくけこ\n");
        assert_eq!(doc.cursor_line(), 1);
        assert_eq!(doc.cursor_col(), 0);
    }

    #[test]
    fn open_line_below_undo() {
        let mut doc = doc_from_str("hello\nworld\n");
        doc.cursors[0] = 3;
        open_line_below(&mut doc);
        // Type some characters in the new line while still in the transaction
        doc.insert_char('x');
        doc.insert_char('y');
        doc.commit_transaction();

        assert_eq!(doc.rope.to_string(), "hello\nxy\nworld\n");

        // A single undo should revert the newline AND the typed characters.
        // Cursor restores to line-end (5) because move_to_line_end runs
        // before begin_transaction, so the transaction records cursor=5.
        assert!(doc.undo());
        assert_eq!(doc.rope.to_string(), "hello\nworld\n");
        assert_eq!(doc.cursors[0], 5);
    }

    #[test]
    fn open_line_below_dirty() {
        let mut doc = doc_from_str("test\n");
        assert!(!doc.dirty);
        open_line_below(&mut doc);
        doc.commit_transaction();
        assert!(doc.dirty);
    }

    #[test]
    fn open_line_below_pending_edits() {
        let mut doc = doc_from_str("test\n");
        assert!(doc.pending_edits.is_empty());
        open_line_below(&mut doc);
        doc.commit_transaction();
        assert!(!doc.pending_edits.is_empty());
    }

    // -------------------------------------------------------
    // Selection
    // -------------------------------------------------------

    #[test]
    fn set_and_clear_anchor() {
        let mut doc = doc_from_str("hello\n");
        doc.cursors[0] = 3;
        doc.set_anchor();
        assert_eq!(doc.selection_anchor(), Some(3));
        doc.clear_anchor();
        assert_eq!(doc.selection_anchor(), None);
    }

    #[test]
    fn shift_extend_right_moves_display_cursor_to_head() {
        let mut doc = doc_from_str("abcd");
        doc.cursors[0] = 0;

        doc.extend_right();

        assert_eq!(doc.display_cursor(), 1);
        assert_eq!(doc.selection_range(), Some((0, 1)));
    }

    #[test]
    fn clear_anchor_after_shift_forward_selection_keeps_cursor_position() {
        let mut doc = doc_from_str("abcd");
        doc.cursors[0] = 0;

        doc.extend_right();
        assert_eq!(doc.cursors[0], 1);
        doc.clear_anchor();

        assert_eq!(doc.cursors[0], 1);
    }

    #[test]
    fn clear_anchor_after_shift_word_forward_keeps_cursor_position() {
        let mut doc = doc_from_str("hello world");
        doc.cursors[0] = 0;

        doc.extend_word_forward_shift();
        assert_eq!(doc.cursors[0], 6);
        assert_eq!(doc.display_cursor(), 6);
        doc.clear_anchor();

        assert_eq!(doc.cursors[0], 6);
    }

    #[test]
    fn selection_range_forward() {
        let mut doc = doc_from_str("hello\n");
        doc.selection = Some(Selection::tail_on_forward(1, 3));
        doc.cursors[0] = 3;
        assert_eq!(doc.selection_range(), Some((1, 3)));
    }

    #[test]
    fn selection_range_backward() {
        let mut doc = doc_from_str("hello\n");
        doc.selection = Some(Selection::tail_on_forward(4, 1));
        doc.cursors[0] = 1;
        assert_eq!(doc.selection_range(), Some((1, 4)));
    }

    #[test]
    fn selection_range_none() {
        let doc = doc_from_str("hello\n");
        assert_eq!(doc.selection_range(), None);
    }

    #[test]
    fn selection_text_basic() {
        let mut doc = doc_from_str("hello world\n");
        doc.selection = Some(Selection::tail_on_forward(0, 5));
        doc.cursors[0] = 4;
        assert_eq!(doc.selection_text(), Some("hello".to_string()));
    }

    #[test]
    fn select_line_basic() {
        let mut doc = doc_from_str("hello\nworld\n");
        doc.cursors[0] = 2;
        doc.select_line();
        assert_eq!(doc.selection_anchor(), Some(0));
        assert_eq!(doc.cursors[0], 6); // one-past '\n' after "hello"
        assert_eq!(doc.selection_text(), Some("hello\n".to_string()));
    }

    #[test]
    fn select_line_includes_newline_via_range() {
        let mut doc = doc_from_str("abc\ndef\n");
        doc.cursors[0] = 1;
        doc.select_line();
        // anchor=0, cursor=3 ('\n'), range = [0, 4) = "abc\n"
        assert_eq!(doc.selection_range(), Some((0, 4)));
    }

    #[test]
    fn select_line_last_line_without_trailing_lf() {
        let mut doc = doc_from_str("top\nlast");
        doc.cursors[0] = 4; // 'l' in "last"
        doc.select_line();
        assert_eq!(doc.selection_range(), Some((4, 8)));
        assert_eq!(doc.selection_text(), Some("last".to_string()));
    }

    #[test]
    fn select_line_empty_line_selects_lf() {
        let mut doc = doc_from_str("\nnext\n");
        doc.cursors[0] = 0;
        doc.select_line();
        assert_eq!(doc.selection_range(), Some((0, 1)));
        assert_eq!(doc.selection_text(), Some("\n".to_string()));
    }

    #[test]
    fn delete_selected_line_removes_line_and_lf() {
        let mut doc = doc_from_str("keep top\nremove me\nkeep end\n");
        doc.cursors[0] = 10; // in "remove me"
        doc.select_line();
        let (start, end) = doc.selection_range().expect("line should be selected");
        let deleted = doc.delete_range(start, end);
        assert_eq!(deleted, "remove me\n");
        assert_eq!(doc.rope.to_string(), "keep top\nkeep end\n");
    }

    #[test]
    fn extend_line_selection_down() {
        let mut doc = doc_from_str("aaa\nbbb\nccc\n");
        doc.select_line(); // selects line 0
        doc.extend_line_selection_down(); // extends to line 1
        assert_eq!(doc.selection_anchor(), Some(0));
        // one-past end of line 1 ("bbb\n"), i.e. start of line 2
        assert_eq!(doc.cursors[0], 8);
    }

    #[test]
    fn delete_range_basic() {
        let mut doc = doc_from_str("hello world\n");
        let deleted = doc.delete_range(5, 11);
        assert_eq!(deleted, " world");
        assert_eq!(doc.rope.to_string(), "hello\n");
        assert_eq!(doc.cursors[0], 5);
    }

    #[test]
    fn delete_range_undo() {
        let mut doc = doc_from_str("hello world\n");
        doc.delete_range(0, 6);
        assert_eq!(doc.rope.to_string(), "world\n");
        assert!(doc.undo());
        assert_eq!(doc.rope.to_string(), "hello world\n");
    }

    #[test]
    fn delete_range_japanese() {
        let mut doc = doc_from_str("あいうえお\n");
        let deleted = doc.delete_range(1, 4);
        assert_eq!(deleted, "いうえ");
        assert_eq!(doc.rope.to_string(), "あお\n");
    }

    #[test]
    fn insert_text_at_basic() {
        let mut doc = doc_from_str("hello\n");
        doc.insert_text_at(5, " world");
        assert_eq!(doc.rope.to_string(), "hello world\n");
        assert_eq!(doc.cursors[0], 11);
    }

    #[test]
    fn insert_text_at_undo() {
        let mut doc = doc_from_str("hello\n");
        doc.insert_text_at(5, " world");
        assert!(doc.undo());
        assert_eq!(doc.rope.to_string(), "hello\n");
    }

    #[test]
    fn insert_text_at_japanese() {
        let mut doc = doc_from_str("あう\n");
        doc.insert_text_at(1, "い");
        assert_eq!(doc.rope.to_string(), "あいう\n");
    }

    #[test]
    fn insert_text_normalizes_crlf_to_lf() {
        let mut doc = doc_from_str("");
        doc.insert_text("a\r\nb");
        assert_eq!(doc.rope.to_string(), "a\nb");
    }

    #[test]
    fn insert_text_normalizes_cr_to_lf() {
        let mut doc = doc_from_str("");
        doc.insert_text("a\rb");
        assert_eq!(doc.rope.to_string(), "a\nb");
    }

    #[test]
    fn insert_text_crlf_undo_redo_roundtrip() {
        let mut doc = doc_from_str("");
        doc.insert_text("x\r\ny");
        assert_eq!(doc.rope.to_string(), "x\ny");
        assert!(doc.undo());
        assert_eq!(doc.rope.to_string(), "");
        assert!(doc.redo());
        assert_eq!(doc.rope.to_string(), "x\ny");
    }

    #[test]
    fn insert_text_at_normalizes_crlf_to_lf() {
        let mut doc = doc_from_str("hello\n");
        doc.insert_text_at(5, "\r\nworld");
        assert_eq!(doc.rope.to_string(), "hello\nworld\n");
    }

    #[test]
    fn insert_text_at_normalizes_cr_to_lf() {
        let mut doc = doc_from_str("ab\n");
        doc.insert_text_at(1, "\r");
        assert_eq!(doc.rope.to_string(), "a\nb\n");
    }

    // -------------------------------------------------------
    // move_to_file_start / move_to_file_end
    // -------------------------------------------------------

    #[test]
    fn move_to_file_start_from_middle() {
        let mut doc = doc_from_str("hello\nworld\n");
        doc.cursors[0] = 8;
        doc.move_to_file_start();
        assert_eq!(doc.cursors[0], 0);
    }

    #[test]
    fn move_to_file_start_already_at_start() {
        let mut doc = doc_from_str("hello\n");
        doc.cursors[0] = 0;
        doc.move_to_file_start();
        assert_eq!(doc.cursors[0], 0);
    }

    #[test]
    fn move_to_file_end_from_start() {
        let mut doc = doc_from_str("hello\nworld\n");
        doc.cursors[0] = 0;
        doc.move_to_file_end();
        assert_eq!(doc.cursors[0], doc.rope.len_chars());
    }

    #[test]
    fn move_to_file_end_already_at_end() {
        let mut doc = doc_from_str("hello\n");
        let end = doc.rope.len_chars();
        doc.cursors[0] = end;
        doc.move_to_file_end();
        assert_eq!(doc.cursors[0], end);
    }

    #[test]
    fn move_to_file_end_empty_doc() {
        let mut doc = doc_from_str("");
        doc.move_to_file_end();
        assert_eq!(doc.cursors[0], 0);
    }

    #[test]
    fn move_to_file_start_japanese() {
        let mut doc = doc_ja();
        doc.cursors[0] = 10;
        doc.move_to_file_start();
        assert_eq!(doc.cursors[0], 0);
    }

    #[test]
    fn move_to_file_end_japanese() {
        let mut doc = doc_ja();
        doc.move_to_file_end();
        assert_eq!(doc.cursors[0], doc.rope.len_chars());
    }

    // -------------------------------------------------------
    // Multi-cursor
    // -------------------------------------------------------

    #[test]
    fn add_cursor_below_basic() {
        let mut doc = doc_from_str("aaa\nbbb\nccc\n");
        doc.cursors[0] = 1; // line 0, col 1
        assert!(doc.add_cursor_below());
        assert_eq!(doc.cursor_count(), 2);
        // Primary cursor unchanged
        assert_eq!(doc.cursors[0], 1);
        // Secondary cursor at line 1, col 1 (char offset 4+1=5)
        assert_eq!(doc.cursors[1], 5);
    }

    #[test]
    fn add_cursor_above_basic() {
        let mut doc = doc_from_str("aaa\nbbb\nccc\n");
        doc.cursors[0] = 5; // line 1, col 1
        assert!(doc.add_cursor_above());
        assert_eq!(doc.cursor_count(), 2);
        // Primary cursor unchanged
        assert_eq!(doc.cursors[0], 5);
        // Secondary cursor at line 0, col 1
        assert_eq!(doc.cursors[1], 1);
    }

    #[test]
    fn add_cursor_at_adds_new_cursor() {
        let mut doc = doc_from_str("hello\nworld\n");
        doc.cursors[0] = 1;
        assert!(doc.add_cursor_at(7));
        assert_eq!(doc.cursor_count(), 2);
        assert_eq!(doc.cursors[0], 1);
        assert!(doc.cursors.contains(&7));
    }

    #[test]
    fn add_cursor_at_dedups_and_preserves_primary() {
        let mut doc = doc_from_str("hello\nworld\n");
        doc.cursors[0] = 7;
        assert!(doc.add_cursor_at(1));
        assert_eq!(doc.cursors[0], 7);
        assert!(!doc.add_cursor_at(1));
        assert_eq!(doc.cursor_count(), 2);
    }

    #[test]
    fn add_cursor_below_clamps_to_shorter_line() {
        let mut doc = doc_from_str("hello\nab\nccc\n");
        doc.cursors[0] = 4; // line 0, col 4 ('o')
        assert!(doc.add_cursor_below());
        assert_eq!(doc.cursor_count(), 2);
        // Line 1 has only 2 chars, so col is clamped to 2
        let expected = doc.rope.line_to_char(1) + 2; // line 1 col 2
        assert_eq!(doc.cursors[1], expected);
    }

    #[test]
    fn add_cursor_above_clamps_to_shorter_line() {
        let mut doc = doc_from_str("ab\nhello\n");
        doc.cursors[0] = doc.rope.line_to_char(1) + 4; // line 1, col 4
        assert!(doc.add_cursor_above());
        assert_eq!(doc.cursor_count(), 2);
        // Line 0 has only 2 chars, so col is clamped to 2
        assert_eq!(doc.cursors[1], 2);
    }

    #[test]
    fn add_cursor_below_fails_at_last_line() {
        let mut doc = doc_from_str("only\n");
        doc.cursors[0] = 2;
        // Move to line 1 (empty line after trailing newline)
        doc.move_down();
        let result = doc.add_cursor_below();
        assert!(!result);
        assert_eq!(doc.cursor_count(), 1);
    }

    #[test]
    fn add_cursor_above_fails_at_first_line() {
        let mut doc = doc_from_str("hello\nworld\n");
        doc.cursors[0] = 2; // line 0
        let result = doc.add_cursor_above();
        assert!(!result);
        assert_eq!(doc.cursor_count(), 1);
    }

    #[test]
    fn remove_secondary_cursors() {
        let mut doc = doc_from_str("aaa\nbbb\nccc\n");
        doc.cursors[0] = 1;
        doc.add_cursor_below();
        doc.add_cursor_below();
        assert_eq!(doc.cursor_count(), 3);
        doc.remove_secondary_cursors();
        assert_eq!(doc.cursor_count(), 1);
        assert_eq!(doc.cursors[0], 1);
    }

    #[test]
    fn multi_cursor_move_right() {
        let mut doc = doc_from_str("aaa\nbbb\n");
        doc.cursors[0] = 0;
        doc.add_cursor_below();
        assert_eq!(doc.cursor_count(), 2);
        doc.move_right();
        assert_eq!(doc.cursors[0], 1);
        assert_eq!(doc.cursors[1], 5); // line_to_char(1) + 1 = 4 + 1 = 5
    }

    #[test]
    fn multi_cursor_insert_char() {
        let mut doc = doc_from_str("aa\nbb\n");
        doc.cursors[0] = 1;
        doc.add_cursor_below();
        // cursors at positions 1 and 4 (line 0 col 1, line 1 col 1)
        doc.insert_char('X');
        // After insert, text should be "aXa\nbXb\n"
        assert_eq!(doc.rope.to_string(), "aXa\nbXb\n");
    }

    #[test]
    fn multi_cursor_delete_backward() {
        let mut doc = doc_from_str("abc\ndef\n");
        doc.cursors[0] = 2; // after 'b'
        doc.add_cursor_below();
        // cursors at positions 2 and 6 (line 0 col 2, line 1 col 2)
        doc.delete_backward();
        // Should delete 'b' and 'e'
        assert_eq!(doc.rope.to_string(), "ac\ndf\n");
    }

    #[test]
    fn cursors_merge_when_they_overlap() {
        let mut doc = doc_from_str("a\nb\n");
        doc.cursors[0] = 0;
        doc.add_cursor_below();
        assert_eq!(doc.cursor_count(), 2);
        // Move both cursors to start of their lines, then keep moving left
        doc.move_to_line_start();
        // Both cursors at col 0, they should still be distinct (line 0 and line 1)
        assert_eq!(doc.cursor_count(), 2);
        // Now move cursor on line 1 up, it should merge with cursor on line 0
        doc.move_up();
        // After merging, should have 1 cursor
        assert_eq!(doc.cursor_count(), 1);
    }

    #[test]
    fn has_multiple_cursors() {
        let mut doc = doc_from_str("aaa\nbbb\n");
        assert!(!doc.has_multiple_cursors());
        doc.add_cursor_below();
        assert!(doc.has_multiple_cursors());
        doc.remove_secondary_cursors();
        assert!(!doc.has_multiple_cursors());
    }

    #[test]
    fn add_cursors_to_top_basic() {
        let mut doc = doc_from_str("aaa\nbbb\nccc\n");
        doc.cursors[0] = doc.rope.line_to_char(2) + 1; // line 2, col 1
        doc.add_cursors_to_top();
        assert_eq!(doc.cursor_count(), 3);
        // Primary stays at index 0
        assert_eq!(doc.cursors[0], doc.rope.line_to_char(2) + 1);
        // Should have cursors on lines 0 and 1 at col 1
        let mut positions = doc.cursors.clone();
        positions.sort();
        assert_eq!(
            positions,
            vec![
                1,
                doc.rope.line_to_char(1) + 1,
                doc.rope.line_to_char(2) + 1
            ]
        );
    }

    #[test]
    fn add_cursors_to_bottom_basic() {
        let mut doc = doc_from_str("aaa\nbbb\nccc\n");
        doc.cursors[0] = 1; // line 0, col 1
        doc.add_cursors_to_bottom();
        // Lines: 0="aaa\n", 1="bbb\n", 2="ccc\n", 3="" (trailing)
        // Should add cursors on lines 1, 2, and 3
        assert_eq!(doc.cursor_count(), 4);
        assert_eq!(doc.cursors[1], doc.rope.line_to_char(1) + 1);
        assert_eq!(doc.cursors[2], doc.rope.line_to_char(2) + 1);
        // Line 3 is empty, col clamped to 0
        assert_eq!(doc.cursors[3], doc.rope.line_to_char(3));
    }

    #[test]
    fn add_cursors_to_top_clamps_columns() {
        let mut doc = doc_from_str("ab\nhello\nxy\n");
        doc.cursors[0] = doc.rope.line_to_char(1) + 4; // line 1, col 4
        doc.add_cursors_to_top();
        assert_eq!(doc.cursor_count(), 2);
        // Line 0 has 2 chars, col clamped to 2
        assert_eq!(doc.cursors[1], 2);
    }

    #[test]
    fn add_cursors_to_bottom_clamps_columns() {
        let mut doc = doc_from_str("hello\nab\nxy\n");
        doc.cursors[0] = 4; // line 0, col 4
        doc.add_cursors_to_bottom();
        // Line 1 has 2 chars -> clamped to col 2
        // Line 2 has 2 chars -> clamped to col 2
        // Line 3 is empty -> clamped to col 0
        assert_eq!(doc.cursor_count(), 4);
        assert_eq!(doc.cursors[1], doc.rope.line_to_char(1) + 2);
        assert_eq!(doc.cursors[2], doc.rope.line_to_char(2) + 2);
        assert_eq!(doc.cursors[3], doc.rope.line_to_char(3));
    }

    #[test]
    fn add_cursors_to_top_already_at_top() {
        let mut doc = doc_from_str("aaa\nbbb\n");
        doc.cursors[0] = 1; // line 0, col 1
        doc.add_cursors_to_top();
        assert_eq!(doc.cursor_count(), 1); // no cursors added
    }

    #[test]
    fn add_cursors_to_bottom_already_at_bottom() {
        let mut doc = doc_from_str("aaa\n");
        // Last line is line 1 (empty after trailing newline)
        doc.cursors[0] = doc.rope.line_to_char(1); // line 1
        doc.add_cursors_to_bottom();
        assert_eq!(doc.cursor_count(), 1); // no cursors added
    }

    #[test]
    fn add_cursors_to_top_with_existing_multi_cursors() {
        let mut doc = doc_from_str("aaa\nbbb\nccc\nddd\n");
        doc.cursors[0] = 1; // line 0, col 1
        doc.add_cursor_below(); // adds cursor on line 1
        doc.add_cursor_below(); // adds cursor on line 2
        assert_eq!(doc.cursor_count(), 3);
        // Now add cursors to top from topmost (line 0) -> nothing to add
        doc.add_cursors_to_top();
        assert_eq!(doc.cursor_count(), 3); // unchanged
    }

    #[test]
    fn multi_cursor_japanese() {
        let mut doc = doc_from_str("あいう\nかきく\n");
        doc.cursors[0] = 1; // after 'あ'
        assert!(doc.add_cursor_below());
        assert_eq!(doc.cursor_count(), 2);
        // Line 1 starts at char 4, col 1 = char 5
        assert_eq!(doc.cursors[1], 5);
        doc.insert_char('ん');
        assert_eq!(doc.rope.to_string(), "あんいう\nかんきく\n");
    }

    #[test]
    fn multi_cursor_word_forward() {
        let mut doc = doc_from_str("hello world\nfoo bar\n");
        doc.cursors[0] = 0; // start of "hello"
        doc.add_cursor_below();
        assert_eq!(doc.cursor_count(), 2);
        // Both cursors at col 0
        doc.move_word_forward();
        // Both should move to the word after first word
        assert_eq!(doc.cursors[0], 6); // start of "world"
        assert_eq!(doc.cursors[1], 16); // start of "bar" (line 1 char 12 + 4)
    }

    #[test]
    fn multi_cursor_word_backward() {
        let mut doc = doc_from_str("hello world\nfoo bar\n");
        doc.cursors[0] = 6; // start of "world"
        doc.cursors.push(16); // start of "bar"
        doc.sort_and_dedup_cursors();
        assert_eq!(doc.cursor_count(), 2);
        doc.move_word_backward();
        // Both should move back one word
        assert_eq!(doc.cursors[0], 0); // start of "hello"
        assert_eq!(doc.cursors[1], 12); // start of "foo"
    }

    #[test]
    fn multi_cursor_word_forward_identical_lines() {
        // Reproduce user-reported bug: cursors at same column on identical lines
        // should move to the same column after word forward motion
        let mut doc = doc_from_str("123 4\n123 4\n");
        doc.cursors[0] = 0; // line 0, col 0
        doc.cursors.push(6); // line 1, col 0
        doc.sort_and_dedup_cursors();
        assert_eq!(doc.cursor_count(), 2);

        doc.move_word_forward();

        // Both should land on '4' (col 4 of their respective lines)
        let col_a = doc.cursors[0] - doc.rope.line_to_char(0); // col on line 0
        let col_b = doc.cursors[1] - doc.rope.line_to_char(1); // col on line 1
        assert_eq!(col_a, col_b, "Cursors should move to same column");
        assert_eq!(col_a, 4, "Should land on '4' at column 4");
    }

    #[test]
    fn clear_anchor_adjusts_all_cursors_forward() {
        let mut doc = doc_from_str("123 4\n123 4\n");
        doc.cursors[0] = 0;
        doc.cursors.push(6);
        doc.sort_and_dedup_cursors();

        // Simulate the normal-mode word-forward flow: set_anchor then move
        doc.set_anchor();
        doc.move_word_forward();

        // Before clear_anchor: raw positions one past display position
        assert_eq!(doc.cursors[0], 4);
        assert_eq!(doc.cursors[1], 10);
        assert!(doc.selection.is_some());

        doc.clear_anchor();

        // After clear_anchor: all cursors adjusted back by 1
        let col_a = doc.cursors[0] - doc.rope.line_to_char(0);
        let col_b = doc.cursors[1] - doc.rope.line_to_char(1);
        assert_eq!(col_a, 3, "Primary should commit to display column");
        assert_eq!(col_b, 3, "Secondary should also be adjusted");
        assert_eq!(col_a, col_b, "Both cursors at same column after clear");
        assert!(doc.selection.is_none());
    }

    #[test]
    fn clear_anchor_no_adjust_for_backward_selection() {
        let mut doc = doc_from_str("123 4\n123 4\n");
        doc.cursors[0] = 4; // on '4' line 0
        doc.cursors.push(10); // on '4' line 1
        doc.sort_and_dedup_cursors();

        doc.set_anchor();
        doc.move_word_backward();

        let pos0 = doc.cursors[0];
        let pos1 = doc.cursors[1];

        doc.clear_anchor();

        // Backward selection: no adjustment
        assert_eq!(doc.cursors[0], pos0);
        assert_eq!(doc.cursors[1], pos1);
    }
}
