use crate::command::git::{GitLineStatus, git_diff_line_status};
use crate::core::buffer::BufferId;
use crate::core::document::{Document, DocumentId};
use crate::core::dot_rec::DotRecorder;
use crate::core::lsp_types::{LspDiagnostic, LspSeverity};
use crate::core::macro_rec::MacroRecorder;
use crate::core::mode::Mode;
use crate::syntax::highlight::HighlightManager;
use crate::syntax::indent;
use crate::syntax::language::LanguageRegistry;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JumpLocation {
    pub doc_id: DocumentId,
    pub file_path: Option<PathBuf>,
    pub cursor: usize,
    pub line: usize,
    pub char_col: usize,
}

pub struct SearchState {
    pub pattern: String,
    pub matches: Vec<usize>,
    pub current_match: Option<usize>,
    /// Cached lowercased document text, reused across keystrokes in a search session.
    lower_text_cache: Option<String>,
    /// All confirmed search patterns (oldest first).
    history: Vec<String>,
    /// Current position when browsing history (`None` = not browsing).
    history_index: Option<usize>,
    /// Saves what the user typed before starting to browse history.
    input_before_history: String,
}

impl Default for SearchState {
    fn default() -> Self {
        Self::new()
    }
}

impl SearchState {
    pub fn new() -> Self {
        Self {
            pattern: String::new(),
            matches: Vec::new(),
            current_match: None,
            lower_text_cache: None,
            history: Vec::new(),
            history_index: None,
            input_before_history: String::new(),
        }
    }

    pub fn clear(&mut self) {
        self.pattern.clear();
        self.matches.clear();
        self.current_match = None;
        self.lower_text_cache = None;
    }

    /// Invalidate the cached lowercased text (call when document content changes).
    pub fn invalidate_cache(&mut self) {
        self.lower_text_cache = None;
    }

    /// Push a non-empty pattern into history, deduplicating consecutive entries.
    pub fn push_history(&mut self, pattern: &str) {
        if pattern.is_empty() {
            return;
        }
        if self.history.last().map(|s| s.as_str()) == Some(pattern) {
            return;
        }
        self.history.push(pattern.to_string());
    }

    /// Move to an older history entry. On the first call, saves `current_input`.
    /// Returns the history entry to display, or `None` if already at the oldest.
    pub fn history_prev(&mut self, current_input: &str) -> Option<String> {
        if self.history.is_empty() {
            return None;
        }
        match self.history_index {
            None => {
                // Start browsing from the newest entry
                self.input_before_history = current_input.to_string();
                let idx = self.history.len() - 1;
                self.history_index = Some(idx);
                Some(self.history[idx].clone())
            }
            Some(0) => {
                // Already at the oldest entry
                None
            }
            Some(idx) => {
                let new_idx = idx - 1;
                self.history_index = Some(new_idx);
                Some(self.history[new_idx].clone())
            }
        }
    }

    /// Move to a newer history entry. Returns the entry to display, or the
    /// saved input when moving past the newest entry back to the user's text.
    /// Returns `None` if not currently browsing history.
    pub fn history_next(&mut self) -> Option<String> {
        let idx = self.history_index?;
        if idx + 1 < self.history.len() {
            let new_idx = idx + 1;
            self.history_index = Some(new_idx);
            Some(self.history[new_idx].clone())
        } else {
            // Past newest → restore user's original input
            self.history_index = None;
            Some(self.input_before_history.clone())
        }
    }

    /// Reset history browsing state (call when opening a new search session).
    pub fn reset_history_browse(&mut self) {
        self.history_index = None;
        self.input_before_history.clear();
    }
}

pub struct Editor {
    documents: Vec<Document>,
    active_index: usize,
    /// MRU file-buffer history (oldest -> newest), excludes scratch buffers.
    buffer_history: Vec<DocumentId>,
    /// Current index for history navigation (`g p` / `g n`).
    buffer_history_index: Option<usize>,
    next_id: DocumentId,
    pub mode: Mode,
    pub message: Option<String>,
    pub highlight_manager: HighlightManager,
    pub language_registry: LanguageRegistry,
    /// Language name per document id (for status bar display).
    language_names: std::collections::HashMap<DocumentId, &'static str>,
    pub search: SearchState,
    /// Single yank register (clipboard-like).
    pub register: Option<String>,
    /// Set when edits occur; cleared after highlights are updated.
    pub highlights_dirty: bool,
    /// Vim-style macro recorder (q/@ commands).
    pub macro_recorder: MacroRecorder,
    /// Dot-repeat recorder (. command).
    pub dot_recorder: DotRecorder,
    diagnostics_by_path: std::collections::HashMap<String, FileDiagnostics>,
    jump_list: Vec<JumpLocation>,
    jump_list_index: Option<usize>,
}

#[derive(Debug, Clone, Default)]
struct FileDiagnostics {
    line_severity: std::collections::HashMap<usize, LspSeverity>,
    line_message: std::collections::HashMap<usize, String>,
}

impl Default for Editor {
    fn default() -> Self {
        Self::new()
    }
}

impl Editor {
    const MAX_JUMP_LIST_LEN: usize = 512;

    pub fn new() -> Self {
        let first = Document::new_scratch(1);
        Self {
            documents: vec![first],
            active_index: 0,
            buffer_history: Vec::new(),
            buffer_history_index: None,
            next_id: 2,
            mode: Mode::Normal,
            message: None,
            highlight_manager: HighlightManager::new(),
            language_registry: LanguageRegistry::new(),
            language_names: std::collections::HashMap::new(),
            search: SearchState::new(),
            register: None,
            highlights_dirty: false,
            macro_recorder: MacroRecorder::new(),
            dot_recorder: DotRecorder::new(),
            diagnostics_by_path: std::collections::HashMap::new(),
            jump_list: Vec::new(),
            jump_list_index: None,
        }
    }

    pub fn open(path: &str) -> Self {
        let language_registry = LanguageRegistry::new();
        let mut highlight_manager = HighlightManager::new();
        let mut language_names = std::collections::HashMap::new();
        let mut first = Document::from_file(1, path);
        first.git_gutter = git_diff_line_status(path);

        if let Some(lang_def) = language_registry.detect_by_extension(path) {
            highlight_manager.register_buffer(first.id, &first.rope, lang_def);
            language_names.insert(first.id, lang_def.name);
        }

        let first_id = first.id;
        Self {
            documents: vec![first],
            active_index: 0,
            buffer_history: vec![first_id],
            buffer_history_index: Some(0),
            next_id: 2,
            mode: Mode::Normal,
            message: None,
            highlight_manager,
            language_registry,
            language_names,
            search: SearchState::new(),
            register: None,
            highlights_dirty: false,
            macro_recorder: MacroRecorder::new(),
            dot_recorder: DotRecorder::new(),
            diagnostics_by_path: std::collections::HashMap::new(),
            jump_list: Vec::new(),
            jump_list_index: None,
        }
    }

    fn locations_equivalent(a: &JumpLocation, b: &JumpLocation) -> bool {
        if let (Some(pa), Some(pb)) = (&a.file_path, &b.file_path) {
            return pa == pb && a.cursor == b.cursor;
        }
        a.doc_id == b.doc_id && a.cursor == b.cursor
    }

    fn trim_jump_list_front(&mut self) {
        if self.jump_list.len() <= Self::MAX_JUMP_LIST_LEN {
            return;
        }
        let drop_count = self.jump_list.len() - Self::MAX_JUMP_LIST_LEN;
        self.jump_list.drain(0..drop_count);
        self.jump_list_index = self
            .jump_list_index
            .map(|idx| idx.saturating_sub(drop_count))
            .or_else(|| {
                if self.jump_list.is_empty() {
                    None
                } else {
                    Some(self.jump_list.len() - 1)
                }
            });
    }

    pub fn current_jump_location(&self) -> JumpLocation {
        let doc = self.active_buffer();
        JumpLocation {
            doc_id: doc.id,
            file_path: doc.file_path.clone(),
            cursor: doc.cursors[0],
            line: doc.cursor_line(),
            char_col: doc.cursor_col(),
        }
    }

    pub fn push_jump_location(&mut self, location: JumpLocation) {
        if let Some(idx) = self.jump_list_index
            && idx + 1 < self.jump_list.len()
        {
            self.jump_list.truncate(idx + 1);
        }

        if self
            .jump_list
            .last()
            .is_some_and(|last| Self::locations_equivalent(last, &location))
        {
            self.jump_list_index = Some(self.jump_list.len().saturating_sub(1));
            return;
        }

        self.jump_list.push(location);
        self.trim_jump_list_front();
        self.jump_list_index = Some(self.jump_list.len().saturating_sub(1));
    }

    pub fn record_jump_transition(&mut self, before: JumpLocation, after: JumpLocation) {
        if Self::locations_equivalent(&before, &after) {
            return;
        }
        self.push_jump_location(before);
        self.push_jump_location(after);
    }

    fn jump_to_location(&mut self, location: &JumpLocation) -> bool {
        if self
            .documents
            .iter()
            .position(|d| d.id == location.doc_id)
            .map(|idx| {
                self.active_index = idx;
                true
            })
            .unwrap_or(false)
        {
            let len = self.active_buffer().rope.len_chars();
            self.active_buffer_mut().cursors = vec![location.cursor.min(len)];
            return true;
        }

        if let Some(path) = &location.file_path {
            self.open_file(&path.to_string_lossy());
            let len = self.active_buffer().rope.len_chars();
            self.active_buffer_mut().cursors = vec![location.cursor.min(len)];
            return true;
        }
        false
    }

    pub fn jump_list_entries(&self) -> &[JumpLocation] {
        &self.jump_list
    }

    pub fn jump_list_index(&self) -> Option<usize> {
        self.jump_list_index
    }

    pub fn jump_to_list_index(&mut self, index: usize) -> Result<(), String> {
        if index >= self.jump_list.len() {
            return Err("Invalid jump location".to_string());
        }
        let location = self.jump_list[index].clone();
        if self.jump_to_location(&location) {
            self.jump_list_index = Some(index);
            Ok(())
        } else {
            Err("Jump location is no longer available".to_string())
        }
    }

    pub fn jump_older(&mut self) -> Result<(), String> {
        let Some(idx) = self.jump_list_index else {
            return Err("Jumplist is empty".to_string());
        };
        if idx == 0 {
            return Err("Already at oldest jump".to_string());
        }
        self.jump_to_list_index(idx - 1)
    }

    pub fn jump_newer(&mut self) -> Result<(), String> {
        let Some(idx) = self.jump_list_index else {
            return Err("Jumplist is empty".to_string());
        };
        if idx + 1 >= self.jump_list.len() {
            return Err("Already at newest jump".to_string());
        }
        self.jump_to_list_index(idx + 1)
    }

    fn is_history_eligible(doc: &Document) -> bool {
        doc.file_path.is_some()
    }

    fn push_active_to_mru(&mut self) {
        let active = &self.documents[self.active_index];
        if !Self::is_history_eligible(active) {
            self.set_history_index_to_active();
            return;
        }
        let active_id = active.id;
        if let Some(pos) = self.buffer_history.iter().position(|id| *id == active_id) {
            self.buffer_history.remove(pos);
        }
        self.buffer_history.push(active_id);
        self.buffer_history_index = Some(self.buffer_history.len() - 1);
    }

    fn set_history_index_to_active(&mut self) {
        let active_id = self.documents[self.active_index].id;
        self.buffer_history_index = self.buffer_history.iter().position(|id| *id == active_id);
    }

    pub fn active_buffer(&self) -> &Document {
        &self.documents[self.active_index]
    }

    pub fn active_buffer_mut(&mut self) -> &mut Document {
        &mut self.documents[self.active_index]
    }

    pub fn set_git_gutter_for_doc(
        &mut self,
        doc_id: DocumentId,
        gutter: HashMap<usize, GitLineStatus>,
    ) {
        if let Some(doc) = self.documents.iter_mut().find(|doc| doc.id == doc_id) {
            doc.git_gutter = gutter;
        }
    }

    pub fn buffers(&self) -> &[Document] {
        &self.documents
    }

    pub fn buffer_by_id(&self, id: BufferId) -> Option<&Document> {
        self.documents.iter().find(|doc| doc.id == id)
    }

    pub fn buffer_count(&self) -> usize {
        self.documents.len()
    }

    #[cfg(test)]
    pub fn active_buffer_id(&self) -> BufferId {
        self.documents[self.active_index].id
    }

    pub fn active_index(&self) -> usize {
        self.active_index
    }

    pub fn new_buffer(&mut self) -> DocumentId {
        let id = self.next_id;
        self.next_id += 1;
        let doc = Document::new_scratch(id);
        self.documents.push(doc);
        self.active_index = self.documents.len() - 1;
        self.set_history_index_to_active();
        id
    }

    pub fn open_file(&mut self, path: &str) {
        // Check if already open
        for (i, doc) in self.documents.iter().enumerate() {
            if let Some(ref fp) = doc.file_path
                && fp.to_str() == Some(path)
            {
                self.active_index = i;
                self.push_active_to_mru();
                return;
            }
        }
        let id = self.next_id;
        self.next_id += 1;
        let mut doc = Document::from_file(id, path);
        doc.git_gutter = git_diff_line_status(path);

        if let Some(lang_def) = self.language_registry.detect_by_extension(path) {
            self.highlight_manager
                .register_buffer(doc.id, &doc.rope, lang_def);
            self.language_names.insert(doc.id, lang_def.name);
        }

        self.documents.push(doc);
        self.active_index = self.documents.len() - 1;
        self.push_active_to_mru();
    }

    pub fn next_buffer(&mut self) {
        if self.documents.len() > 1 {
            self.active_index = (self.active_index + 1) % self.documents.len();
        }
    }

    pub fn prev_buffer(&mut self) {
        if self.documents.len() > 1 {
            if self.active_index == 0 {
                self.active_index = self.documents.len() - 1;
            } else {
                self.active_index -= 1;
            }
        }
    }

    pub fn switch_to_index(&mut self, index: usize) -> bool {
        if index < self.documents.len() {
            self.active_index = index;
            self.push_active_to_mru();
            true
        } else {
            false
        }
    }

    pub fn switch_to_buffer(&mut self, id: BufferId) -> bool {
        if let Some(idx) = self.documents.iter().position(|d| d.id == id) {
            self.active_index = idx;
            self.push_active_to_mru();
            true
        } else {
            false
        }
    }

    /// Move to the previous (older) entry in buffer MRU history.
    /// Returns true when the active buffer changed.
    pub fn prev_buffer_history(&mut self) -> bool {
        if self.buffer_history.is_empty() {
            return false;
        }

        let active_id = self.documents[self.active_index].id;
        let active_in_history = self.buffer_history.iter().position(|id| *id == active_id);
        let target_idx = match active_in_history {
            None => self.buffer_history.len() - 1,
            Some(0) => {
                self.buffer_history_index = Some(0);
                return false;
            }
            Some(idx) => idx - 1,
        };

        let target_id = self.buffer_history[target_idx];
        if let Some(doc_idx) = self.documents.iter().position(|d| d.id == target_id) {
            self.active_index = doc_idx;
            self.buffer_history_index = Some(target_idx);
            true
        } else {
            false
        }
    }

    /// Move to the next (newer) entry in buffer MRU history.
    /// Returns true when the active buffer changed.
    pub fn next_buffer_history(&mut self) -> bool {
        if self.buffer_history.is_empty() {
            return false;
        }

        let active_id = self.documents[self.active_index].id;
        let Some(current_idx) = self.buffer_history.iter().position(|id| *id == active_id) else {
            return false;
        };
        if current_idx + 1 >= self.buffer_history.len() {
            self.buffer_history_index = Some(current_idx);
            return false;
        }

        let target_idx = current_idx + 1;
        let target_id = self.buffer_history[target_idx];
        if let Some(doc_idx) = self.documents.iter().position(|d| d.id == target_id) {
            self.active_index = doc_idx;
            self.buffer_history_index = Some(target_idx);
            true
        } else {
            false
        }
    }

    /// Mark that highlights need updating (deferred to next render).
    pub fn mark_highlights_dirty(&mut self) {
        self.highlights_dirty = true;
        self.search.invalidate_cache();
    }

    /// Drain pending edits from the active document and update highlighting.
    pub fn update_highlights(&mut self) {
        self.highlights_dirty = false;
        let doc = &mut self.documents[self.active_index];
        if doc.pending_edits.is_empty() {
            return;
        }
        let edits: Vec<_> = doc.pending_edits.drain(..).collect();
        let doc_id = doc.id;
        let rope = &doc.rope;
        self.highlight_manager.update(doc_id, rope, &edits);
    }

    /// Update highlights only if marked dirty. Call before rendering.
    pub fn update_highlights_if_dirty(&mut self) {
        if self.highlights_dirty {
            self.update_highlights();
        }
    }

    /// Register highlights for the active buffer by file extension.
    /// Useful from external code (e.g. benchmarks) where splitting borrows is awkward.
    pub fn register_highlights_for_extension(&mut self, ext: &str) {
        let doc = &self.documents[self.active_index];
        let doc_id = doc.id;
        if let Some(lang_def) = self.language_registry.detect_by_extension(ext) {
            self.highlight_manager.register_buffer(
                doc_id,
                &self.documents[self.active_index].rope,
                lang_def,
            );
            self.language_names.insert(doc_id, lang_def.name);
        }
    }

    /// Refresh highlight registration and language name for the active buffer's current file path.
    pub fn refresh_active_buffer_language(&mut self) {
        let doc_id = self.documents[self.active_index].id;
        let path = self.documents[self.active_index]
            .file_path
            .as_ref()
            .and_then(|p| p.to_str())
            .map(str::to_owned);

        match path
            .as_deref()
            .and_then(|p| self.language_registry.detect_by_extension(p))
        {
            Some(lang_def) => {
                let rope = &self.documents[self.active_index].rope;
                self.highlight_manager
                    .register_buffer(doc_id, rope, lang_def);
                self.language_names.insert(doc_id, lang_def.name);
            }
            None => {
                self.highlight_manager.unregister_buffer(doc_id);
                self.language_names.remove(&doc_id);
            }
        }
    }

    /// Get the detected language name for the active document.
    pub fn active_language_name(&self) -> Option<&'static str> {
        let doc_id = self.documents[self.active_index].id;
        self.language_names.get(&doc_id).copied()
    }

    fn prune_stale_search_matches_for_active_buffer(&mut self) {
        if self.search.matches.is_empty() {
            return;
        }

        let text_len = self.documents[self.active_index].rope.len_chars();
        let pattern_len = self.search.pattern.chars().count();
        if pattern_len == 0 || text_len == 0 {
            self.search.matches.clear();
            self.search.current_match = None;
            return;
        }

        self.search
            .matches
            .retain(|&m| m < text_len && m.saturating_add(pattern_len) <= text_len);

        if let Some(idx) = self.search.current_match
            && idx >= self.search.matches.len()
        {
            self.search.current_match = None;
        }
    }

    /// Update search pattern and find all matches (case-insensitive) in the active buffer.
    pub fn search_update(&mut self, pattern: &str) {
        self.search.pattern = pattern.to_string();
        self.search.matches.clear();
        self.search.current_match = None;

        if pattern.is_empty() {
            return;
        }

        let lower_pattern = pattern.to_lowercase();

        if lower_pattern.is_empty() {
            return;
        }

        // Reuse cached lowercased text if available (same document during a search session)
        if self.search.lower_text_cache.is_none() {
            let text = self.documents[self.active_index].rope.to_string();
            self.search.lower_text_cache = Some(text.to_lowercase());
        }
        let lower_text = self.search.lower_text_cache.as_ref().unwrap();

        // Use str::find for efficient byte-level search, then convert byte
        // offsets to char offsets incrementally.
        let pat_byte_len = lower_pattern.len();
        let pat_char_len = lower_pattern.chars().count();
        let mut byte_pos = 0;
        let mut char_pos = 0;

        while byte_pos + pat_byte_len <= lower_text.len() {
            match lower_text[byte_pos..].find(&*lower_pattern) {
                Some(rel) => {
                    let match_byte = byte_pos + rel;
                    // Count chars in the skipped segment
                    char_pos += lower_text[byte_pos..match_byte].chars().count();
                    self.search.matches.push(char_pos);
                    char_pos += pat_char_len;
                    byte_pos = match_byte + pat_byte_len;
                }
                None => break,
            }
        }
    }

    /// Move cursor to the next search match after current cursor position. Wraps around.
    pub fn search_next(&mut self) {
        self.prune_stale_search_matches_for_active_buffer();
        if self.search.matches.is_empty() {
            return;
        }
        let cursor = self.documents[self.active_index].cursors[0];
        // Find first match with offset > cursor
        let idx = self.search.matches.iter().position(|&m| m > cursor);
        let idx = idx.unwrap_or(0); // wrap to first match
        self.search.current_match = Some(idx);
        self.documents[self.active_index].cursors[0] = self.search.matches[idx];
    }

    /// Move cursor to the previous search match before current cursor position. Wraps around.
    pub fn search_prev(&mut self) {
        self.prune_stale_search_matches_for_active_buffer();
        if self.search.matches.is_empty() {
            return;
        }
        let cursor = self.documents[self.active_index].cursors[0];
        // Find last match with offset < cursor
        let idx = self.search.matches.iter().rposition(|&m| m < cursor);
        let idx = idx.unwrap_or(self.search.matches.len() - 1); // wrap to last match
        self.search.current_match = Some(idx);
        self.documents[self.active_index].cursors[0] = self.search.matches[idx];
    }

    /// Add a secondary cursor to the next search match.
    /// Skips already-used cursor locations and wraps around.
    pub fn add_cursor_to_next_search_match(&mut self) -> bool {
        self.prune_stale_search_matches_for_active_buffer();
        if self.search.matches.is_empty() {
            return false;
        }
        let match_len = self.search.pattern.chars().count();
        if match_len == 0 {
            return false;
        }
        let cursor = self.documents[self.active_index].cursors[0];
        let len = self.search.matches.len();
        let start_idx = self
            .search
            .matches
            .iter()
            .position(|&m| m > cursor)
            .unwrap_or(0);
        for step in 0..len {
            let idx = (start_idx + step) % len;
            let pos = self.search.matches[idx];
            let end = pos.saturating_add(match_len);
            let occupied = self.documents[self.active_index]
                .cursors
                .iter()
                .any(|&cursor_pos| cursor_pos >= pos && cursor_pos < end);
            if occupied {
                continue;
            }
            if self.documents[self.active_index].add_cursor_at(pos) {
                self.search.current_match = Some(idx);
                return true;
            }
        }
        false
    }

    /// Add a secondary cursor to the previous search match.
    /// Skips already-used cursor locations and wraps around.
    pub fn add_cursor_to_prev_search_match(&mut self) -> bool {
        self.prune_stale_search_matches_for_active_buffer();
        if self.search.matches.is_empty() {
            return false;
        }
        let match_len = self.search.pattern.chars().count();
        if match_len == 0 {
            return false;
        }
        let cursor = self.documents[self.active_index].cursors[0];
        let len = self.search.matches.len();
        let start_idx = self
            .search
            .matches
            .iter()
            .rposition(|&m| m < cursor)
            .unwrap_or(len - 1);
        for step in 0..len {
            let idx = (start_idx + len - step) % len;
            let pos = self.search.matches[idx];
            let end = pos.saturating_add(match_len);
            let occupied = self.documents[self.active_index]
                .cursors
                .iter()
                .any(|&cursor_pos| cursor_pos >= pos && cursor_pos < end);
            if occupied {
                continue;
            }
            if self.documents[self.active_index].add_cursor_at(pos) {
                self.search.current_match = Some(idx);
                return true;
            }
        }
        false
    }

    /// Insert a newline with tree-sitter-based auto-indent.
    /// Falls back to copying the current line's indent when no tree/indent query is available.
    pub fn insert_newline_with_indent(&mut self, tab_width: usize) {
        // Ensure tree is up-to-date before reading it for indent calculation
        self.update_highlights_if_dirty();
        let doc = &self.documents[self.active_index];
        let doc_id = doc.id;
        let cursor = doc.cursors[0].min(doc.rope.len_chars());
        let cursor_byte = doc.rope.char_to_byte(cursor);
        let current_line = doc.cursor_line();

        // Try tree-sitter indent calculation
        let indent_str = if let (Some(tree), Some(iq)) = (
            self.highlight_manager.tree(doc_id),
            self.highlight_manager.indent_query(doc_id),
        ) {
            let source = doc.rope.to_string();
            let level = indent::calculate_indent_level(tree, iq, source.as_bytes(), cursor_byte);
            indent::indent_string(level, tab_width)
        } else {
            indent::copy_line_indent(&doc.rope, current_line)
        };

        let text = format!("\n{}", indent_str);
        self.documents[self.active_index].insert_text(&text);
        self.mark_highlights_dirty();
    }

    pub fn close_active_buffer(&mut self) -> Result<(), String> {
        if self.documents[self.active_index].dirty {
            return Err("Buffer has unsaved changes".to_string());
        }
        self.force_close_active_buffer();
        Ok(())
    }

    /// Close the active buffer without checking for unsaved changes.
    pub fn force_close_active_buffer(&mut self) {
        let old_id = self.documents[self.active_index].id;
        self.highlight_manager.unregister_buffer(old_id);
        self.language_names.remove(&old_id);
        self.buffer_history.retain(|id| *id != old_id);

        if self.documents.len() == 1 {
            // Replace with a new scratch document
            let id = self.next_id;
            self.next_id += 1;
            self.documents[0] = Document::new_scratch(id);
            self.active_index = 0;
        } else {
            self.documents.remove(self.active_index);
            if self.active_index >= self.documents.len() {
                self.active_index = self.documents.len() - 1;
            }
        }
        self.set_history_index_to_active();
    }

    /// Returns true when there is exactly one buffer that is a clean scratch buffer.
    pub fn is_single_clean_scratch(&self) -> bool {
        self.documents.len() == 1
            && self.documents[0].file_path.is_none()
            && !self.documents[0].dirty
    }

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_editor_has_one_scratch_buffer() {
        let ed = Editor::new();
        assert_eq!(ed.buffer_count(), 1);
        assert_eq!(ed.active_buffer().display_name(), "[scratch]");
    }

    #[test]
    fn new_buffer_increments_id() {
        let mut ed = Editor::new();
        let id1 = ed.active_buffer_id();
        assert_eq!(id1, 1);
        let id2 = ed.new_buffer();
        assert_eq!(id2, 2);
        assert_eq!(ed.buffer_count(), 2);
        assert_eq!(ed.active_buffer_id(), 2);
    }

    #[test]
    fn next_prev_buffer_cycles() {
        let mut ed = Editor::new();
        ed.new_buffer();
        ed.new_buffer();
        // active is buffer 3 (index 2)
        assert_eq!(ed.active_index(), 2);

        ed.next_buffer();
        assert_eq!(ed.active_index(), 0); // wraps

        ed.prev_buffer();
        assert_eq!(ed.active_index(), 2); // wraps back
    }

    #[test]
    fn open_file_reopen_promotes_to_mru() {
        let mut ed = Editor::new();
        ed.open_file("foo.txt");
        ed.open_file("bar.txt");
        assert_eq!(ed.buffer_history, vec![2, 3]);

        ed.open_file("foo.txt");
        assert_eq!(ed.buffer_count(), 3);
        assert_eq!(ed.active_buffer_id(), 2);
        assert_eq!(ed.buffer_history, vec![3, 2]);
    }

    #[test]
    fn manual_switch_promotes_to_mru() {
        let mut ed = Editor::new();
        ed.open_file("a.txt");
        ed.open_file("b.txt");
        ed.open_file("c.txt");
        assert_eq!(ed.buffer_history, vec![2, 3, 4]);

        assert!(ed.switch_to_index(2)); // b.txt
        assert_eq!(ed.active_buffer_id(), 3);
        assert_eq!(ed.buffer_history, vec![2, 4, 3]);
    }

    #[test]
    fn prev_next_buffer_history_navigates_without_reordering() {
        let mut ed = Editor::new();
        ed.open_file("a.txt");
        ed.open_file("b.txt");
        ed.open_file("c.txt");
        let history_before = ed.buffer_history.clone();

        assert!(ed.prev_buffer_history());
        assert_eq!(ed.active_buffer_id(), 3);
        assert_eq!(ed.buffer_history, history_before);

        assert!(ed.prev_buffer_history());
        assert_eq!(ed.active_buffer_id(), 2);
        assert_eq!(ed.buffer_history, history_before);

        assert!(ed.next_buffer_history());
        assert_eq!(ed.active_buffer_id(), 3);
        assert_eq!(ed.buffer_history, history_before);
    }

    #[test]
    fn scratch_buffer_is_excluded_from_history() {
        let mut ed = Editor::new();
        ed.open_file("a.txt");
        assert_eq!(ed.buffer_history, vec![2]);

        let scratch_id = ed.new_buffer();
        assert_eq!(ed.active_buffer_id(), scratch_id);
        assert_eq!(ed.buffer_history, vec![2]);

        assert!(ed.prev_buffer_history());
        assert_eq!(ed.active_buffer_id(), 2);
        assert!(!ed.next_buffer_history());
    }

    #[test]
    fn close_buffer_removes_history_entry_and_navigation_skips_it() {
        let mut ed = Editor::new();
        ed.open_file("a.txt");
        ed.open_file("b.txt");
        ed.open_file("c.txt");
        assert_eq!(ed.buffer_history, vec![2, 3, 4]);

        assert!(ed.switch_to_buffer(3));
        ed.force_close_active_buffer();
        assert_eq!(ed.buffer_history, vec![2, 4]);
        assert!(!ed.buffer_history.contains(&3));

        assert!(ed.prev_buffer_history());
        assert_eq!(ed.active_buffer_id(), 2);
        assert!(ed.next_buffer_history());
        assert_eq!(ed.active_buffer_id(), 4);
    }

    #[test]
    fn switch_to_buffer_by_id() {
        let mut ed = Editor::new();
        ed.new_buffer();
        ed.new_buffer();
        assert!(ed.switch_to_buffer(1));
        assert_eq!(ed.active_index(), 0);
        assert!(!ed.switch_to_buffer(999)); // nonexistent
    }

    #[test]
    fn close_buffer_removes_it() {
        let mut ed = Editor::new();
        ed.new_buffer();
        assert_eq!(ed.buffer_count(), 2);
        ed.close_active_buffer().unwrap();
        assert_eq!(ed.buffer_count(), 1);
    }

    #[test]
    fn close_last_buffer_replaces_with_scratch() {
        let mut ed = Editor::new();
        ed.close_active_buffer().unwrap();
        assert_eq!(ed.buffer_count(), 1);
        assert_eq!(ed.active_buffer().display_name(), "[scratch]");
    }

    #[test]
    fn close_dirty_buffer_fails() {
        let mut ed = Editor::new();
        ed.active_buffer_mut().dirty = true;
        let result = ed.close_active_buffer();
        assert!(result.is_err());
    }

    #[test]
    fn open_file_deduplicates() {
        let mut ed = Editor::new();
        ed.open_file("foo.txt");
        ed.open_file("bar.txt");
        assert_eq!(ed.buffer_count(), 3);
        ed.open_file("foo.txt"); // should not create new buffer
        assert_eq!(ed.buffer_count(), 3);
    }

    #[test]
    fn jumplist_records_transition_and_navigates() {
        let mut ed = editor_with_text("a\nb\nc\n");
        ed.active_buffer_mut().set_cursor_line_char(0, 0);
        let before = ed.current_jump_location();
        ed.active_buffer_mut().set_cursor_line_char(2, 0);
        let after = ed.current_jump_location();

        ed.record_jump_transition(before, after);
        assert_eq!(ed.jump_list_entries().len(), 2);
        assert_eq!(ed.jump_list_index(), Some(1));

        ed.jump_older().unwrap();
        assert_eq!(ed.active_buffer().cursor_line(), 0);
        assert_eq!(ed.jump_list_index(), Some(0));

        ed.jump_newer().unwrap();
        assert_eq!(ed.active_buffer().cursor_line(), 2);
        assert_eq!(ed.jump_list_index(), Some(1));
    }

    #[test]
    fn jumplist_deduplicates_equivalent_locations() {
        let mut ed = editor_with_text("hello\n");
        let loc = ed.current_jump_location();
        ed.push_jump_location(loc.clone());
        ed.push_jump_location(loc);
        assert_eq!(ed.jump_list_entries().len(), 1);
    }

    #[test]
    fn jumplist_rejects_invalid_index() {
        let mut ed = Editor::new();
        let err = ed.jump_to_list_index(0).unwrap_err();
        assert!(err.contains("Invalid jump location"));
    }

    // -------------------------------------------------------
    // Search helpers
    // -------------------------------------------------------

    fn editor_with_text(s: &str) -> Editor {
        let mut ed = Editor::new();
        ed.active_buffer_mut().rope = ropey::Rope::from_str(s);
        ed
    }

    // -------------------------------------------------------
    // search_update tests
    // -------------------------------------------------------

    #[test]
    fn search_update_finds_matches() {
        let mut ed = editor_with_text("hello world hello");
        ed.search_update("hello");
        assert_eq!(ed.search.matches, vec![0, 12]);
    }

    #[test]
    fn search_update_case_insensitive() {
        let mut ed = editor_with_text("Hello HELLO hello");
        ed.search_update("hello");
        assert_eq!(ed.search.matches.len(), 3);
    }

    #[test]
    fn search_update_empty_pattern() {
        let mut ed = editor_with_text("hello world");
        ed.search_update("");
        assert!(ed.search.matches.is_empty());
    }

    #[test]
    fn search_update_no_match() {
        let mut ed = editor_with_text("hello world");
        ed.search_update("xyz");
        assert!(ed.search.matches.is_empty());
    }

    #[test]
    fn search_update_japanese() {
        let mut ed = editor_with_text("竹取の翁といふものありけり");
        ed.search_update("翁");
        assert_eq!(ed.search.matches.len(), 1);
        assert_eq!(ed.search.matches[0], 3); // char offset of '翁'
    }

    #[test]
    fn search_update_non_overlapping() {
        let mut ed = editor_with_text("aaa");
        ed.search_update("aa");
        // Non-overlapping: only [0], since after matching at 0 we skip to 2
        assert_eq!(ed.search.matches, vec![0]);
    }

    // -------------------------------------------------------
    // search_next tests
    // -------------------------------------------------------

    #[test]
    fn search_next_moves_to_first_match_after_cursor() {
        let mut ed = editor_with_text("hello world hello test hello");
        ed.search_update("hello");
        ed.active_buffer_mut().cursors[0] = 0;
        ed.search_next();
        assert_eq!(ed.active_buffer().cursors[0], 12);
    }

    #[test]
    fn search_next_wraps_around() {
        let mut ed = editor_with_text("hello world hello");
        ed.search_update("hello");
        ed.active_buffer_mut().cursors[0] = 16;
        ed.search_next();
        assert_eq!(ed.active_buffer().cursors[0], 0); // wraps to first
    }

    #[test]
    fn search_next_no_matches() {
        let mut ed = editor_with_text("hello world");
        ed.search_update("xyz");
        ed.active_buffer_mut().cursors[0] = 0;
        ed.search_next();
        assert_eq!(ed.active_buffer().cursors[0], 0); // unchanged
    }

    #[test]
    fn search_next_prunes_stale_out_of_bounds_matches() {
        let mut ed = editor_with_text("short");
        ed.search.pattern = "hello".to_string();
        ed.search.matches = vec![3303];
        ed.active_buffer_mut().cursors[0] = 0;

        ed.search_next();

        assert_eq!(ed.active_buffer().cursors[0], 0);
        assert!(ed.search.matches.is_empty());
        assert_eq!(ed.search.current_match, None);
    }

    #[test]
    fn search_next_from_match_position() {
        let mut ed = editor_with_text("hello world hello");
        ed.search_update("hello");
        // matches = [0, 12]; cursor at 0, search_next should go to the match > 0
        ed.active_buffer_mut().cursors[0] = 0;
        ed.search_next();
        assert_eq!(ed.active_buffer().cursors[0], 12);
    }

    // -------------------------------------------------------
    // search_prev tests
    // -------------------------------------------------------

    #[test]
    fn search_prev_moves_to_last_match_before_cursor() {
        let mut ed = editor_with_text("hello world hello test hello");
        ed.search_update("hello");
        // matches = [0, 12, 23]
        ed.active_buffer_mut().cursors[0] = 13;
        ed.search_prev();
        assert_eq!(ed.active_buffer().cursors[0], 12);
    }

    #[test]
    fn search_prev_wraps_around() {
        let mut ed = editor_with_text("hello world hello");
        ed.search_update("hello");
        // matches = [0, 12]; cursor at 0, prev wraps to last
        ed.active_buffer_mut().cursors[0] = 0;
        ed.search_prev();
        assert_eq!(ed.active_buffer().cursors[0], 12);
    }

    #[test]
    fn search_prev_no_matches() {
        let mut ed = editor_with_text("hello world");
        ed.search_update("xyz");
        ed.active_buffer_mut().cursors[0] = 5;
        ed.search_prev();
        assert_eq!(ed.active_buffer().cursors[0], 5); // unchanged
    }

    #[test]
    fn add_cursor_to_next_search_match_adds_secondary_cursor() {
        let mut ed = editor_with_text("hello world hello");
        ed.search_update("hello");
        ed.active_buffer_mut().cursors[0] = 0;
        assert!(ed.add_cursor_to_next_search_match());
        assert_eq!(ed.active_buffer().cursors, vec![0, 12]);
    }

    #[test]
    fn add_cursor_to_prev_search_match_wraps_and_adds_secondary_cursor() {
        let mut ed = editor_with_text("hello world hello");
        ed.search_update("hello");
        ed.active_buffer_mut().cursors[0] = 0;
        assert!(ed.add_cursor_to_prev_search_match());
        assert_eq!(ed.active_buffer().cursors, vec![0, 12]);
    }

    #[test]
    fn add_cursor_to_next_search_match_skips_existing_cursor_positions() {
        let mut ed = editor_with_text("hello world hello test hello");
        ed.search_update("hello");
        ed.active_buffer_mut().cursors[0] = 0;
        assert!(ed.add_cursor_to_next_search_match());
        assert!(ed.add_cursor_to_next_search_match());
        assert!(!ed.add_cursor_to_next_search_match());
        let mut sorted = ed.active_buffer().cursors.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, vec![0, 12, 23]);
    }

    #[test]
    fn add_cursor_to_next_search_match_treats_cursor_inside_match_as_occupied() {
        let mut ed = editor_with_text("hello world hello");
        ed.search_update("hello");
        // Primary cursor is inside the first match.
        ed.active_buffer_mut().cursors[0] = 2;
        assert!(ed.add_cursor_to_next_search_match());
        assert!(!ed.add_cursor_to_next_search_match());
        let mut sorted = ed.active_buffer().cursors.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, vec![2, 12]);
    }

    #[test]
    fn add_cursor_to_prev_search_match_treats_cursor_inside_match_as_occupied() {
        let mut ed = editor_with_text("hello world hello");
        ed.search_update("hello");
        // Primary cursor is inside the last match.
        ed.active_buffer_mut().cursors[0] = 14;
        assert!(ed.add_cursor_to_prev_search_match());
        assert!(!ed.add_cursor_to_prev_search_match());
        let mut sorted = ed.active_buffer().cursors.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, vec![0, 14]);
    }

    // -------------------------------------------------------
    // Integration tests
    // -------------------------------------------------------

    #[test]
    fn search_next_then_prev_round_trip() {
        let mut ed = editor_with_text("aaa bbb aaa bbb aaa");
        ed.search_update("bbb");
        // matches = [4, 12]
        ed.active_buffer_mut().cursors[0] = 0;
        ed.search_next();
        assert_eq!(ed.active_buffer().cursors[0], 4);
        ed.search_next();
        assert_eq!(ed.active_buffer().cursors[0], 12);
        ed.search_prev();
        assert_eq!(ed.active_buffer().cursors[0], 4);
    }

    #[test]
    fn search_update_clears_old_matches() {
        let mut ed = editor_with_text("hello world foo");
        ed.search_update("hello");
        assert_eq!(ed.search.matches.len(), 1);
        ed.search_update("foo");
        assert_eq!(ed.search.matches.len(), 1);
        assert_eq!(ed.search.matches[0], 12);
    }

    // -------------------------------------------------------
    // Search history tests
    // -------------------------------------------------------

    #[test]
    fn push_history_stores_entries() {
        let mut s = SearchState::new();
        s.push_history("foo");
        s.push_history("bar");
        assert_eq!(s.history, vec!["foo", "bar"]);
    }

    #[test]
    fn push_history_deduplicates_consecutive() {
        let mut s = SearchState::new();
        s.push_history("foo");
        s.push_history("foo");
        s.push_history("bar");
        s.push_history("bar");
        s.push_history("foo");
        assert_eq!(s.history, vec!["foo", "bar", "foo"]);
    }

    #[test]
    fn push_history_ignores_empty() {
        let mut s = SearchState::new();
        s.push_history("");
        assert!(s.history.is_empty());
    }

    #[test]
    fn history_prev_navigates_older() {
        let mut s = SearchState::new();
        s.push_history("alpha");
        s.push_history("beta");
        s.push_history("gamma");

        // First prev → newest entry
        assert_eq!(s.history_prev("current"), Some("gamma".to_string()));
        assert_eq!(s.history_prev("current"), Some("beta".to_string()));
        assert_eq!(s.history_prev("current"), Some("alpha".to_string()));
        // At oldest → None
        assert_eq!(s.history_prev("current"), None);
    }

    #[test]
    fn history_next_navigates_newer() {
        let mut s = SearchState::new();
        s.push_history("alpha");
        s.push_history("beta");

        // Go to oldest
        s.history_prev("typed");
        s.history_prev("typed");
        assert_eq!(s.history_index, Some(0));

        // Navigate forward
        assert_eq!(s.history_next(), Some("beta".to_string()));
        // Past newest → restore saved input
        assert_eq!(s.history_next(), Some("typed".to_string()));
        assert_eq!(s.history_index, None);
    }

    #[test]
    fn history_next_returns_none_when_not_browsing() {
        let mut s = SearchState::new();
        s.push_history("foo");
        assert_eq!(s.history_next(), None);
    }

    #[test]
    fn history_prev_returns_none_when_empty() {
        let mut s = SearchState::new();
        assert_eq!(s.history_prev("anything"), None);
    }

    #[test]
    fn input_before_history_preserved() {
        let mut s = SearchState::new();
        s.push_history("old1");
        s.push_history("old2");

        // Start browsing with user text "partial"
        s.history_prev("partial");
        assert_eq!(s.input_before_history, "partial");

        // Navigate back to user text
        s.history_prev("partial"); // → old1
        s.history_next(); // → old2
        s.history_next(); // → "partial"
        assert_eq!(s.history_next(), None); // not browsing anymore
    }

    #[test]
    fn reset_history_browse_clears_state() {
        let mut s = SearchState::new();
        s.push_history("foo");
        s.history_prev("bar");
        assert!(s.history_index.is_some());
        s.reset_history_browse();
        assert_eq!(s.history_index, None);
        assert!(s.input_before_history.is_empty());
    }
}
