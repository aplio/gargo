use super::*;

impl Editor {
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

    pub fn reset_search(&mut self) {
        self.search.clear();
    }
}
