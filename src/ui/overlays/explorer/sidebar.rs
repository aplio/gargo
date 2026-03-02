use std::collections::HashMap;
use std::fs::OpenOptions;
use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::command::git::{GitFileStatus, dir_git_status};
use crate::input::action::{Action, AppAction, BufferAction, IntegrationAction, WorkspaceAction};
use crate::input::chord::KeyState;
use crate::ui::framework::cell::CellStyle;
use crate::ui::framework::component::EventResult;
use crate::ui::framework::surface::Surface;
use crate::ui::shared::file_browser::{is_valid_single_name, sort_by_name_case_insensitive};
use crate::ui::shared::filtering::fuzzy_match;
use crate::ui::text_input::delete_prev_word_input;
use crate::ui::text::truncate_to_width;

struct DirEntry {
    name: String,
    is_dir: bool,
    git_status: Option<GitFileStatus>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ExplorerMode {
    AllFiles,
    ChangedOnly,
}

pub struct Explorer {
    mode: ExplorerMode,
    current_dir: PathBuf,
    entries: Vec<DirEntry>,
    visible_entries: Vec<usize>,
    selected: usize,
    scroll_offset: usize,
    find_active: bool,
    find_input: String,
    copy_menu_active: bool,
    rename_active: bool,
    rename_input: String,
    add_active: bool,
    add_input: String,
    delete_confirm_active: bool,
    project_root: PathBuf,
    git_status_map: HashMap<String, GitFileStatus>,
}

impl Explorer {
    pub fn new(
        dir: PathBuf,
        project_root: &Path,
        git_status_map: &HashMap<String, GitFileStatus>,
    ) -> Self {
        Self::new_with_mode(dir, project_root, git_status_map, ExplorerMode::AllFiles)
    }

    pub fn new_changed_only(
        dir: PathBuf,
        project_root: &Path,
        git_status_map: &HashMap<String, GitFileStatus>,
    ) -> Self {
        Self::new_with_mode(dir, project_root, git_status_map, ExplorerMode::ChangedOnly)
    }

    fn new_with_mode(
        dir: PathBuf,
        project_root: &Path,
        git_status_map: &HashMap<String, GitFileStatus>,
        mode: ExplorerMode,
    ) -> Self {
        let mut explorer = Self {
            mode,
            current_dir: dir,
            entries: Vec::new(),
            visible_entries: Vec::new(),
            selected: 0,
            scroll_offset: 0,
            find_active: false,
            find_input: String::new(),
            copy_menu_active: false,
            rename_active: false,
            rename_input: String::new(),
            add_active: false,
            add_input: String::new(),
            delete_confirm_active: false,
            project_root: project_root.to_path_buf(),
            git_status_map: git_status_map.clone(),
        };
        explorer.read_directory();
        explorer
    }

    fn read_directory(&mut self) {
        self.entries.clear();
        self.visible_entries.clear();
        self.selected = 0;
        self.scroll_offset = 0;
        self.find_active = false;
        self.find_input.clear();
        self.copy_menu_active = false;
        self.rename_active = false;
        self.rename_input.clear();
        self.add_active = false;
        self.add_input.clear();
        self.delete_confirm_active = false;

        if self.mode == ExplorerMode::ChangedOnly {
            self.read_changed_entries();
            return;
        }

        let mut dirs = Vec::new();
        let mut files = Vec::new();

        if let Ok(read_dir) = std::fs::read_dir(&self.current_dir) {
            for entry in read_dir.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                // Skip dotfiles
                if name.starts_with('.') {
                    continue;
                }
                let is_dir = entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false);
                let full_path = entry.path();
                let rel_path = full_path
                    .strip_prefix(&self.project_root)
                    .unwrap_or(&full_path)
                    .to_string_lossy()
                    .to_string();

                let git_status = if is_dir {
                    let prefix = if rel_path.ends_with('/') {
                        rel_path.clone()
                    } else {
                        format!("{}/", rel_path)
                    };
                    dir_git_status(&self.git_status_map, &prefix)
                } else {
                    self.git_status_map.get(&rel_path).copied()
                };

                if is_dir {
                    dirs.push(DirEntry {
                        name,
                        is_dir: true,
                        git_status,
                    });
                } else {
                    files.push(DirEntry {
                        name,
                        is_dir: false,
                        git_status,
                    });
                }
            }
        }

        sort_by_name_case_insensitive(&mut dirs, |entry| &entry.name);
        sort_by_name_case_insensitive(&mut files, |entry| &entry.name);

        self.entries.extend(dirs);
        self.entries.extend(files);

        self.visible_entries = (0..self.entries.len()).collect();
    }

    fn read_changed_entries(&mut self) {
        let mut files: Vec<DirEntry> = self
            .git_status_map
            .iter()
            .map(|(path, status)| DirEntry {
                name: path.clone(),
                is_dir: false,
                git_status: Some(*status),
            })
            .collect();
        sort_by_name_case_insensitive(&mut files, |entry| &entry.name);
        self.entries.extend(files);
        self.visible_entries = (0..self.entries.len()).collect();
    }

    pub fn handle_key(&mut self, key: KeyEvent, key_state: &KeyState) -> EventResult {
        // When a chord is in progress, yield so the chord resolves
        if *key_state != KeyState::Normal {
            return EventResult::Ignored;
        }

        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('q') {
            return EventResult::Action(Action::App(AppAction::Workspace(
                WorkspaceAction::ToggleExplorer,
            )));
        }

        if self.copy_menu_active {
            return self.handle_copy_menu_key(key);
        }

        if self.rename_active {
            return self.handle_rename_key(key);
        }

        if self.add_active {
            return self.handle_add_key(key);
        }

        if self.delete_confirm_active {
            return self.handle_delete_confirm_key(key);
        }

        if self.find_active {
            return self.handle_find_key(key);
        }

        if key.modifiers.contains(KeyModifiers::CONTROL) {
            return match key.code {
                KeyCode::Char('n') => {
                    self.move_down();
                    EventResult::Consumed
                }
                KeyCode::Char('p') => {
                    self.move_up();
                    EventResult::Consumed
                }
                KeyCode::Char('f') => {
                    return self.enter_selected();
                }
                KeyCode::Char('b') => {
                    self.go_parent();
                    EventResult::Consumed
                }
                _ => EventResult::Ignored,
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
            KeyCode::Char('h') | KeyCode::Left => {
                self.go_parent();
                EventResult::Consumed
            }
            KeyCode::Char('l') | KeyCode::Right | KeyCode::Enter => self.enter_selected(),
            KeyCode::Char('/') => {
                self.find_active = true;
                self.find_input.clear();
                EventResult::Consumed
            }
            KeyCode::Char('c') => {
                self.copy_menu_active = true;
                EventResult::Consumed
            }
            KeyCode::Char('r') => self.start_rename_prompt(),
            KeyCode::Char('a') => self.start_add_prompt(),
            KeyCode::Char('d') => self.start_delete_confirm(),
            KeyCode::Esc => EventResult::Action(Action::App(AppAction::Workspace(
                WorkspaceAction::ToggleExplorer,
            ))),
            KeyCode::Char(' ') => EventResult::Ignored, // let Space chord start
            _ => EventResult::Consumed,
        }
    }

    pub fn set_git_status_map(&mut self, git_status_map: &HashMap<String, GitFileStatus>) {
        self.git_status_map = git_status_map.clone();

        if self.mode == ExplorerMode::ChangedOnly {
            let selected_name = self.selected_name().map(ToString::to_string);
            self.read_directory();
            if let Some(name) = selected_name {
                self.select_by_name(&name);
            }
            return;
        }

        let statuses: Vec<Option<GitFileStatus>> = self
            .entries
            .iter()
            .map(|entry| self.entry_git_status(&entry.name, entry.is_dir))
            .collect();
        for (entry, status) in self.entries.iter_mut().zip(statuses) {
            entry.git_status = status;
        }
    }

    fn entry_git_status(&self, entry_name: &str, is_dir: bool) -> Option<GitFileStatus> {
        let full_path = self.current_dir.join(entry_name);
        let rel_path = full_path
            .strip_prefix(&self.project_root)
            .unwrap_or(&full_path)
            .to_string_lossy()
            .to_string();

        if is_dir {
            let prefix = if rel_path.ends_with('/') {
                rel_path
            } else {
                format!("{}/", rel_path)
            };
            dir_git_status(&self.git_status_map, &prefix)
        } else {
            self.git_status_map.get(&rel_path).copied()
        }
    }

    fn handle_copy_menu_key(&mut self, key: KeyEvent) -> EventResult {
        self.copy_menu_active = false;
        match key.code {
            KeyCode::Char('c') => self.copy_selected_full_path(),
            KeyCode::Char('d') => self.copy_selected_dir_path(),
            KeyCode::Char('f') => self.copy_selected_name(),
            _ => EventResult::Consumed,
        }
    }

    fn handle_find_key(&mut self, key: KeyEvent) -> EventResult {
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            return match key.code {
                KeyCode::Char('n') => {
                    self.move_down();
                    EventResult::Consumed
                }
                KeyCode::Char('p') => {
                    self.move_up();
                    EventResult::Consumed
                }
                KeyCode::Char('f') => self.enter_selected(),
                KeyCode::Char('b') => {
                    self.go_parent();
                    EventResult::Consumed
                }
                KeyCode::Char('w') => {
                    self.delete_prev_word();
                    self.jump_to_best_match();
                    EventResult::Consumed
                }
                KeyCode::Char('k') => {
                    self.find_input.clear();
                    EventResult::Consumed
                }
                KeyCode::Char('u') => {
                    self.find_input.clear();
                    EventResult::Consumed
                }
                _ => EventResult::Consumed,
            };
        }

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
                self.jump_to_best_match();
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
            KeyCode::Left => {
                self.go_parent();
                EventResult::Consumed
            }
            KeyCode::Right => self.enter_selected(),
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::ALT) => {
                self.find_input.push(c);
                self.jump_to_best_match();
                EventResult::Consumed
            }
            _ => EventResult::Consumed,
        }
    }

    fn handle_rename_key(&mut self, key: KeyEvent) -> EventResult {
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            return match key.code {
                KeyCode::Char('w') => {
                    delete_prev_word_input(&mut self.rename_input);
                    EventResult::Consumed
                }
                KeyCode::Char('u') | KeyCode::Char('k') => {
                    self.rename_input.clear();
                    EventResult::Consumed
                }
                _ => EventResult::Consumed,
            };
        }

        match key.code {
            KeyCode::Esc => {
                self.rename_active = false;
                self.rename_input.clear();
                EventResult::Consumed
            }
            KeyCode::Enter => {
                self.rename_active = false;
                self.apply_rename()
            }
            KeyCode::Backspace => {
                self.rename_input.pop();
                EventResult::Consumed
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::ALT) => {
                self.rename_input.push(c);
                EventResult::Consumed
            }
            _ => EventResult::Consumed,
        }
    }

    fn handle_add_key(&mut self, key: KeyEvent) -> EventResult {
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            return match key.code {
                KeyCode::Char('w') => {
                    delete_prev_word_input(&mut self.add_input);
                    EventResult::Consumed
                }
                KeyCode::Char('u') | KeyCode::Char('k') => {
                    self.add_input.clear();
                    EventResult::Consumed
                }
                _ => EventResult::Consumed,
            };
        }

        match key.code {
            KeyCode::Esc => {
                self.add_active = false;
                self.add_input.clear();
                EventResult::Consumed
            }
            KeyCode::Enter => {
                self.add_active = false;
                self.apply_add()
            }
            KeyCode::Backspace => {
                self.add_input.pop();
                EventResult::Consumed
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::ALT) => {
                self.add_input.push(c);
                EventResult::Consumed
            }
            _ => EventResult::Consumed,
        }
    }

    fn handle_delete_confirm_key(&mut self, key: KeyEvent) -> EventResult {
        self.delete_confirm_active = false;
        match key.code {
            KeyCode::Char('y') => self.apply_delete(),
            _ => self.show_message("Delete aborted".to_string()),
        }
    }

    fn jump_to_best_match(&mut self) {
        if self.find_input.is_empty() {
            return;
        }
        let mut best: Option<(i32, usize)> = None;
        for (visible_idx, &entry_idx) in self.visible_entries.iter().enumerate() {
            if let Some((score, _)) = fuzzy_match(&self.entries[entry_idx].name, &self.find_input)
                && best.is_none_or(|(best_score, _)| score > best_score)
            {
                best = Some((score, visible_idx));
            }
        }
        if let Some((_, visible_idx)) = best {
            self.selected = visible_idx;
        }
    }

    fn delete_prev_word(&mut self) {
        delete_prev_word_input(&mut self.find_input);
    }

    fn show_message(&self, message: String) -> EventResult {
        EventResult::Action(Action::App(AppAction::Integration(
            IntegrationAction::ShowMessage(message),
        )))
    }

    fn start_rename_prompt(&mut self) -> EventResult {
        let Some(name) = self.selected_entry().map(|entry| entry.name.clone()) else {
            return EventResult::Consumed;
        };
        self.rename_active = true;
        self.rename_input = name;
        EventResult::Consumed
    }

    fn start_add_prompt(&mut self) -> EventResult {
        self.add_active = true;
        self.add_input.clear();
        EventResult::Consumed
    }

    fn start_delete_confirm(&mut self) -> EventResult {
        if self.selected_entry().is_none() {
            return EventResult::Consumed;
        }
        self.delete_confirm_active = true;
        EventResult::Consumed
    }

    fn apply_rename(&mut self) -> EventResult {
        let Some(entry) = self.selected_entry() else {
            return self.show_message("Rename failed: no selection".to_string());
        };
        let source_name = entry.name.clone();
        let source_path = self.current_dir.join(&source_name);
        let new_name = self.rename_input.trim().to_string();
        if !is_valid_single_name(&new_name) {
            return self.show_message("Rename failed: invalid name".to_string());
        }
        if new_name == source_name {
            return self.show_message("Rename skipped: unchanged".to_string());
        }
        let dest_path = self.current_dir.join(&new_name);
        if dest_path.exists() {
            return self.show_message(format!("Rename failed: '{}' already exists", new_name));
        }
        match std::fs::rename(&source_path, &dest_path) {
            Ok(()) => {
                self.read_directory();
                self.select_by_name(&new_name);
                self.show_message(format!("Renamed to {}", new_name))
            }
            Err(e) => self.show_message(format!("Rename failed: {}", e)),
        }
    }

    fn apply_add(&mut self) -> EventResult {
        let raw = self.add_input.trim().to_string();
        let is_dir = raw.ends_with('/');
        let name = raw.trim_end_matches('/');
        if !is_valid_single_name(name) {
            return self.show_message("Add failed: invalid name".to_string());
        }

        let target = self.current_dir.join(name);
        if target.exists() {
            return self.show_message(format!("Add failed: '{}' already exists", name));
        }

        let result = if is_dir {
            std::fs::create_dir(&target)
        } else {
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&target)
                .map(|_| ())
        };

        match result {
            Ok(()) => {
                self.read_directory();
                self.select_by_name(name);
                let kind = if is_dir { "directory" } else { "file" };
                self.show_message(format!("Created {} {}", kind, name))
            }
            Err(e) => self.show_message(format!("Add failed: {}", e)),
        }
    }

    fn apply_delete(&mut self) -> EventResult {
        if self.visible_entries.is_empty() {
            return self.show_message("Delete failed: no selection".to_string());
        }
        let entry_idx = self.visible_entries[self.selected];
        let entry_name = self.entries[entry_idx].name.clone();
        let entry_is_dir = self.entries[entry_idx].is_dir;
        let target = self.current_dir.join(&entry_name);
        let old_selected = self.selected;

        let result = if entry_is_dir {
            std::fs::remove_dir_all(&target)
        } else {
            std::fs::remove_file(&target)
        };

        match result {
            Ok(()) => {
                self.read_directory();
                if !self.visible_entries.is_empty() {
                    self.selected = old_selected.min(self.visible_entries.len() - 1);
                }
                self.show_message(format!("Deleted {}", entry_name))
            }
            Err(e) => self.show_message(format!("Delete failed: {}", e)),
        }
    }

    fn move_down(&mut self) {
        if !self.visible_entries.is_empty() && self.selected + 1 < self.visible_entries.len() {
            self.selected += 1;
        }
    }

    fn move_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    fn go_parent(&mut self) {
        if self.mode == ExplorerMode::ChangedOnly {
            return;
        }
        if let Some(parent) = self.current_dir.parent() {
            let old_name = self
                .current_dir
                .file_name()
                .map(|n| n.to_string_lossy().to_string());
            self.current_dir = parent.to_path_buf();
            self.read_directory();
            if let Some(name) = old_name {
                self.select_by_name(&name);
            }
        }
    }

    fn enter_selected(&mut self) -> EventResult {
        if self.visible_entries.is_empty() {
            return EventResult::Consumed;
        }
        let entry_idx = self.visible_entries[self.selected];
        let entry = &self.entries[entry_idx];
        if entry.is_dir {
            let new_dir = self.current_dir.join(&entry.name);
            self.current_dir = new_dir;
            self.read_directory();
            EventResult::Consumed
        } else {
            let path = self.current_dir.join(&entry.name);
            let path_str = path.to_string_lossy().to_string();
            EventResult::Action(Action::App(AppAction::Buffer(
                BufferAction::OpenFileFromExplorer(path_str),
            )))
        }
    }

    fn selected_entry(&self) -> Option<&DirEntry> {
        if self.visible_entries.is_empty() {
            return None;
        }
        let idx = self.visible_entries[self.selected];
        self.entries.get(idx)
    }

    fn copy_selected_full_path(&self) -> EventResult {
        let Some(entry) = self.selected_entry() else {
            return EventResult::Consumed;
        };
        let path = self.current_dir.join(&entry.name);
        EventResult::Action(Action::App(AppAction::Integration(
            IntegrationAction::CopyToClipboard {
                text: path.to_string_lossy().to_string(),
                description: "path".to_string(),
            },
        )))
    }

    fn copy_selected_dir_path(&self) -> EventResult {
        let Some(entry) = self.selected_entry() else {
            return EventResult::Consumed;
        };
        let path = if entry.is_dir {
            self.current_dir.join(&entry.name)
        } else {
            self.current_dir.clone()
        };
        EventResult::Action(Action::App(AppAction::Integration(
            IntegrationAction::CopyToClipboard {
                text: path.to_string_lossy().to_string(),
                description: "dir path".to_string(),
            },
        )))
    }

    fn copy_selected_name(&self) -> EventResult {
        let Some(entry) = self.selected_entry() else {
            return EventResult::Consumed;
        };
        EventResult::Action(Action::App(AppAction::Integration(
            IntegrationAction::CopyToClipboard {
                text: entry.name.clone(),
                description: "file name".to_string(),
            },
        )))
    }

    pub fn select_by_name(&mut self, name: &str) {
        for (i, &idx) in self.visible_entries.iter().enumerate() {
            if self.entries[idx].name == name {
                self.selected = i;
                return;
            }
        }
    }

    pub fn current_dir(&self) -> &Path {
        &self.current_dir
    }

    pub fn is_changed_only(&self) -> bool {
        self.mode == ExplorerMode::ChangedOnly
    }

    pub fn selected_name(&self) -> Option<&str> {
        if self.visible_entries.is_empty() {
            return None;
        }
        let idx = self.visible_entries[self.selected];
        Some(&self.entries[idx].name)
    }

    pub fn render(&mut self, surface: &mut Surface, x: usize, width: usize, height: usize) {
        if width == 0 || height == 0 {
            return;
        }

        let default_style = CellStyle::default();
        let dim_style = CellStyle {
            dim: true,
            ..CellStyle::default()
        };

        // Header: show current directory path
        let header = self.truncated_path_header(width);
        surface.put_str(x, 0, &header, &dim_style);
        let header_w = crate::ui::text::display_width(&header);
        if header_w < width {
            surface.fill_region(x + header_w, 0, width - header_w, ' ', &dim_style);
        }

        // Compute content area: rows 1..height (reserve row 0 for header)
        // If prompt is active, reserve the last row for prompt
        let content_start_row = 1;
        let bottom_prompt_active = self.find_active
            || self.copy_menu_active
            || self.rename_active
            || self.add_active
            || self.delete_confirm_active;
        let content_height = if bottom_prompt_active {
            height.saturating_sub(2) // header + find prompt
        } else {
            height.saturating_sub(1) // header only
        };

        // Adjust scroll offset to keep selected visible
        if self.selected < self.scroll_offset {
            self.scroll_offset = self.selected;
        }
        if self.selected >= self.scroll_offset + content_height {
            self.scroll_offset = self
                .selected
                .saturating_sub(content_height.saturating_sub(1));
        }

        // Draw entries
        for row in 0..content_height {
            let vis_idx = self.scroll_offset + row;
            let screen_row = content_start_row + row;
            if screen_row >= height {
                break;
            }

            if vis_idx < self.visible_entries.len() {
                let entry_idx = self.visible_entries[vis_idx];
                let entry = &self.entries[entry_idx];
                let is_selected = vis_idx == self.selected;

                let prefix = if is_selected { "> " } else { "  " };
                let display = if self.mode == ExplorerMode::ChangedOnly {
                    let status = entry.git_status.map_or(' ', |s| s.indicator());
                    format!("{}[{}] {}", prefix, status, entry.name)
                } else {
                    let suffix = if entry.is_dir { "/" } else { "" };
                    format!("{}{}{}", prefix, entry.name, suffix)
                };

                let style = if is_selected {
                    CellStyle {
                        reverse: true,
                        fg: entry.git_status.map(|s| s.color()),
                        ..CellStyle::default()
                    }
                } else {
                    CellStyle {
                        fg: entry.git_status.map(|s| s.color()),
                        ..CellStyle::default()
                    }
                };
                let (truncated, used) = truncate_to_width(&display, width);
                surface.put_str(x, screen_row, truncated, &style);
                if used < width {
                    surface.fill_region(x + used, screen_row, width - used, ' ', &style);
                }
            } else {
                // Empty row
                surface.fill_region(x, screen_row, width, ' ', &default_style);
            }
        }

        // Bottom prompt
        if bottom_prompt_active {
            let find_row = height.saturating_sub(1);
            let prompt = self.bottom_prompt();
            let find_style = CellStyle {
                reverse: true,
                ..CellStyle::default()
            };
            let (truncated, used) = truncate_to_width(&prompt, width);
            surface.put_str(x, find_row, truncated, &find_style);
            if used < width {
                surface.fill_region(x + used, find_row, width - used, ' ', &find_style);
            }
        }
    }

    fn truncated_path_header(&self, max_width: usize) -> String {
        let path = &self.current_dir;
        let components: Vec<_> = path
            .components()
            .map(|c| c.as_os_str().to_string_lossy().to_string())
            .collect();

        if components.is_empty() {
            return "/".to_string();
        }

        // Try building from the last component backwards
        // Start with just the last dir name + /
        let last = &components[components.len() - 1];
        let mut result = format!("{}/", last);

        if crate::ui::text::display_width(&result) <= max_width {
            // Try adding more parent components
            for i in (0..components.len() - 1).rev() {
                let candidate = format!("{}/{}", components[i], result);
                if crate::ui::text::display_width(&candidate) <= max_width {
                    result = candidate;
                } else {
                    break;
                }
            }
        }

        // If even the last component doesn't fit, truncate it
        if crate::ui::text::display_width(&result) > max_width {
            let (truncated, _) = truncate_to_width(&result, max_width);
            return truncated.to_string();
        }

        result
    }

    fn bottom_prompt(&self) -> String {
        if self.find_active {
            format!("/{}", self.find_input)
        } else if self.copy_menu_active {
            "copy: [c] path [d] dir [f] name".to_string()
        } else if self.rename_active {
            format!("rename: {}", self.rename_input)
        } else if self.add_active {
            format!("add: {} (end with / for dir)", self.add_input)
        } else if self.delete_confirm_active {
            let label = self.selected_name().unwrap_or("item");
            format!("delete {}? [y/N]", label)
        } else {
            String::new()
        }
    }

    /// Returns cursor position (x, y) for the find prompt, if find is active
    pub fn find_cursor(&self, x: usize, height: usize) -> Option<(u16, u16)> {
        let prompt = if self.find_active {
            format!("/{}", self.find_input)
        } else if self.rename_active {
            format!("rename: {}", self.rename_input)
        } else if self.add_active {
            format!("add: {} (end with / for dir)", self.add_input)
        } else {
            String::new()
        };

        if prompt.is_empty() {
            return None;
        }
        let find_row = height.saturating_sub(1);
        let cursor_x = x + crate::ui::text::display_width(&prompt);
        Some((cursor_x as u16, find_row as u16))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn ctrl_key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    fn setup(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("gargo_test_explorer_{}", name));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::create_dir_all(dir.join("aaa_dir")).unwrap();
        fs::write(dir.join("bbb.txt"), "bbb").unwrap();
        fs::write(dir.join("ccc.rs"), "ccc").unwrap();
        dir
    }

    fn cleanup(dir: &Path) {
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn changed_only_mode_shows_only_changed_entries() {
        let dir = setup("changed_only");
        let mut git_status_map = HashMap::new();
        git_status_map.insert("bbb.txt".to_string(), GitFileStatus::Modified);
        git_status_map.insert("aaa_dir/nested.txt".to_string(), GitFileStatus::Added);

        let explorer = Explorer::new_changed_only(dir.clone(), &dir, &git_status_map);

        assert!(explorer.is_changed_only());
        assert_eq!(explorer.visible_entries.len(), 2);
        let names: Vec<String> = explorer
            .visible_entries
            .iter()
            .map(|&idx| explorer.entries[idx].name.clone())
            .collect();
        assert_eq!(
            names,
            vec!["aaa_dir/nested.txt".to_string(), "bbb.txt".to_string()]
        );
        assert!(explorer.entries.iter().all(|entry| !entry.is_dir));

        cleanup(&dir);
    }

    #[test]
    fn changed_only_mode_enter_opens_nested_path_as_file() {
        let dir = setup("changed_open_nested");
        fs::write(dir.join("aaa_dir").join("nested.txt"), "nested").unwrap();
        let mut git_status_map = HashMap::new();
        git_status_map.insert("aaa_dir/nested.txt".to_string(), GitFileStatus::Modified);
        let mut explorer = Explorer::new_changed_only(dir.clone(), &dir, &git_status_map);

        let result = explorer.handle_key(key(KeyCode::Enter), &KeyState::Normal);
        match result {
            EventResult::Action(Action::App(AppAction::Buffer(
                BufferAction::OpenFileFromExplorer(path),
            ))) => {
                assert_eq!(PathBuf::from(path), dir.join("aaa_dir").join("nested.txt"));
            }
            _ => panic!("Expected OpenFileFromExplorer action"),
        }

        cleanup(&dir);
    }

    #[test]
    fn changed_only_mode_renders_status_badge() {
        let dir = setup("changed_badge");
        let mut git_status_map = HashMap::new();
        git_status_map.insert("bbb.txt".to_string(), GitFileStatus::Modified);
        let mut explorer = Explorer::new_changed_only(dir.clone(), &dir, &git_status_map);
        let mut surface = Surface::new(40, 6);

        explorer.render(&mut surface, 0, 40, 6);

        let row: String = (0..40)
            .map(|x| {
                let symbol = &surface.get(x, 1).symbol;
                if symbol.is_empty() {
                    ' '
                } else {
                    symbol.chars().next().unwrap_or(' ')
                }
            })
            .collect();
        assert!(
            row.contains("[M] bbb.txt"),
            "row did not contain status badge: {}",
            row
        );

        cleanup(&dir);
    }

    #[test]
    fn find_mode_jumps_selection_without_filtering() {
        let dir = setup("find_jump");
        let mut explorer = Explorer::new(dir.clone(), &dir, &HashMap::new());

        explorer.handle_key(key(KeyCode::Char('/')), &KeyState::Normal);
        explorer.handle_key(key(KeyCode::Char('c')), &KeyState::Normal);
        explorer.handle_key(key(KeyCode::Char('c')), &KeyState::Normal);
        explorer.handle_key(key(KeyCode::Char('c')), &KeyState::Normal);

        assert_eq!(explorer.visible_entries.len(), 3);
        assert_eq!(explorer.selected_name(), Some("ccc.rs"));

        cleanup(&dir);
    }

    #[test]
    fn find_mode_ctrl_and_arrow_navigation_work() {
        let dir = setup("find_nav");
        let mut explorer = Explorer::new(dir.clone(), &dir, &HashMap::new());

        explorer.handle_key(key(KeyCode::Char('/')), &KeyState::Normal);
        explorer.handle_key(ctrl_key('n'), &KeyState::Normal);
        assert_eq!(explorer.selected_name(), Some("bbb.txt"));
        explorer.handle_key(ctrl_key('p'), &KeyState::Normal);
        assert_eq!(explorer.selected_name(), Some("aaa_dir"));
        explorer.handle_key(key(KeyCode::Down), &KeyState::Normal);
        assert_eq!(explorer.selected_name(), Some("bbb.txt"));
        explorer.handle_key(key(KeyCode::Up), &KeyState::Normal);
        assert_eq!(explorer.selected_name(), Some("aaa_dir"));

        cleanup(&dir);
    }

    #[test]
    fn find_mode_ctrl_w_ctrl_u_and_ctrl_k_edit_query() {
        let dir = setup("find_ctrl_edit");
        let mut explorer = Explorer::new(dir.clone(), &dir, &HashMap::new());

        explorer.handle_key(key(KeyCode::Char('/')), &KeyState::Normal);
        for c in "src ui ccc".chars() {
            explorer.handle_key(key(KeyCode::Char(c)), &KeyState::Normal);
        }
        explorer.handle_key(ctrl_key('w'), &KeyState::Normal);
        assert_eq!(explorer.find_input, "src ui ");
        explorer.handle_key(ctrl_key('u'), &KeyState::Normal);
        assert!(explorer.find_input.is_empty());
        for c in "tmp new".chars() {
            explorer.handle_key(key(KeyCode::Char(c)), &KeyState::Normal);
        }
        explorer.handle_key(ctrl_key('k'), &KeyState::Normal);
        assert!(explorer.find_input.is_empty());

        cleanup(&dir);
    }

    #[test]
    fn copy_menu_cc_copies_selected_full_path() {
        let dir = setup("copy_path");
        let mut explorer = Explorer::new(dir.clone(), &dir, &HashMap::new());
        explorer.handle_key(key(KeyCode::Char('j')), &KeyState::Normal); // bbb.txt

        let _ = explorer.handle_key(key(KeyCode::Char('c')), &KeyState::Normal);
        let result = explorer.handle_key(key(KeyCode::Char('c')), &KeyState::Normal);

        match result {
            EventResult::Action(Action::App(AppAction::Integration(
                IntegrationAction::CopyToClipboard { text, description },
            ))) => {
                assert!(text.ends_with("bbb.txt"));
                assert_eq!(description, "path");
            }
            _ => panic!("Expected CopyToClipboard path action"),
        }

        cleanup(&dir);
    }

    #[test]
    fn copy_menu_cd_copies_directory_path() {
        let dir = setup("copy_dir");
        let mut explorer = Explorer::new(dir.clone(), &dir, &HashMap::new());
        explorer.handle_key(key(KeyCode::Char('j')), &KeyState::Normal); // bbb.txt

        let _ = explorer.handle_key(key(KeyCode::Char('c')), &KeyState::Normal);
        let result = explorer.handle_key(key(KeyCode::Char('d')), &KeyState::Normal);

        match result {
            EventResult::Action(Action::App(AppAction::Integration(
                IntegrationAction::CopyToClipboard { text, description },
            ))) => {
                assert_eq!(PathBuf::from(text), dir);
                assert_eq!(description, "dir path");
            }
            _ => panic!("Expected CopyToClipboard dir path action"),
        }

        cleanup(&dir);
    }

    #[test]
    fn copy_menu_cf_copies_file_name() {
        let dir = setup("copy_name");
        let mut explorer = Explorer::new(dir.clone(), &dir, &HashMap::new());
        explorer.handle_key(key(KeyCode::Char('j')), &KeyState::Normal); // bbb.txt

        let _ = explorer.handle_key(key(KeyCode::Char('c')), &KeyState::Normal);
        let result = explorer.handle_key(key(KeyCode::Char('f')), &KeyState::Normal);

        match result {
            EventResult::Action(Action::App(AppAction::Integration(
                IntegrationAction::CopyToClipboard { text, description },
            ))) => {
                assert_eq!(text, "bbb.txt");
                assert_eq!(description, "file name");
            }
            _ => panic!("Expected CopyToClipboard file name action"),
        }

        cleanup(&dir);
    }

    #[test]
    fn copy_menu_invalid_second_key_is_consumed_and_closes_menu() {
        let dir = setup("copy_invalid");
        let mut explorer = Explorer::new(dir.clone(), &dir, &HashMap::new());

        let _ = explorer.handle_key(key(KeyCode::Char('c')), &KeyState::Normal);
        let first = explorer.handle_key(key(KeyCode::Char('x')), &KeyState::Normal);
        assert!(matches!(first, EventResult::Consumed));

        let second = explorer.handle_key(key(KeyCode::Char('j')), &KeyState::Normal);
        assert!(matches!(second, EventResult::Consumed));
        assert_eq!(explorer.selected_name(), Some("bbb.txt"));

        cleanup(&dir);
    }

    #[test]
    fn rename_selected_file_with_r() {
        let dir = setup("rename_file");
        let mut explorer = Explorer::new(dir.clone(), &dir, &HashMap::new());
        explorer.handle_key(key(KeyCode::Char('j')), &KeyState::Normal); // bbb.txt

        let _ = explorer.handle_key(key(KeyCode::Char('r')), &KeyState::Normal);
        let _ = explorer.handle_key(ctrl_key('u'), &KeyState::Normal);
        for c in "renamed.txt".chars() {
            let _ = explorer.handle_key(key(KeyCode::Char(c)), &KeyState::Normal);
        }
        let result = explorer.handle_key(key(KeyCode::Enter), &KeyState::Normal);

        assert!(dir.join("renamed.txt").exists());
        assert!(!dir.join("bbb.txt").exists());
        assert_eq!(explorer.selected_name(), Some("renamed.txt"));
        assert!(matches!(
            result,
            EventResult::Action(Action::App(AppAction::Integration(
                IntegrationAction::ShowMessage(_),
            )))
        ));

        cleanup(&dir);
    }

    #[test]
    fn add_file_and_dir_with_a() {
        let dir = setup("add_entries");
        let mut explorer = Explorer::new(dir.clone(), &dir, &HashMap::new());

        let _ = explorer.handle_key(key(KeyCode::Char('a')), &KeyState::Normal);
        for c in "new.txt".chars() {
            let _ = explorer.handle_key(key(KeyCode::Char(c)), &KeyState::Normal);
        }
        let _ = explorer.handle_key(key(KeyCode::Enter), &KeyState::Normal);
        assert!(dir.join("new.txt").exists());
        assert_eq!(explorer.selected_name(), Some("new.txt"));

        let _ = explorer.handle_key(key(KeyCode::Char('a')), &KeyState::Normal);
        for c in "new_dir/".chars() {
            let _ = explorer.handle_key(key(KeyCode::Char(c)), &KeyState::Normal);
        }
        let _ = explorer.handle_key(key(KeyCode::Enter), &KeyState::Normal);
        assert!(dir.join("new_dir").is_dir());
        assert_eq!(explorer.selected_name(), Some("new_dir"));

        cleanup(&dir);
    }

    #[test]
    fn delete_confirmation_requires_y() {
        let dir = setup("delete_confirm");
        let mut explorer = Explorer::new(dir.clone(), &dir, &HashMap::new());
        explorer.handle_key(key(KeyCode::Char('j')), &KeyState::Normal); // bbb.txt

        let _ = explorer.handle_key(key(KeyCode::Char('d')), &KeyState::Normal);
        let _ = explorer.handle_key(key(KeyCode::Char('n')), &KeyState::Normal);
        assert!(dir.join("bbb.txt").exists());

        let _ = explorer.handle_key(key(KeyCode::Char('d')), &KeyState::Normal);
        let _ = explorer.handle_key(key(KeyCode::Char('y')), &KeyState::Normal);
        assert!(!dir.join("bbb.txt").exists());

        cleanup(&dir);
    }
}
