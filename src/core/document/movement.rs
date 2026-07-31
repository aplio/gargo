use super::*;

impl Document {
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

    /// Move each cursor one *display* line down (vim's `gj`): with soft wrap on
    /// this steps to the next wrapped row of the same line when there is one,
    /// keeping the column within the row. Falls back to [`Document::move_down`]
    /// when the text is not wrapped.
    pub fn move_down_display(&mut self, text_width: usize) {
        if text_width == 0 {
            self.move_down();
            return;
        }
        self.move_by_display_row(text_width, true);
    }

    /// Move each cursor one display line up (vim's `gk`). See
    /// [`Document::move_down_display`].
    pub fn move_up_display(&mut self, text_width: usize) {
        if text_width == 0 {
            self.move_up();
            return;
        }
        self.move_by_display_row(text_width, false);
    }

    fn move_by_display_row(&mut self, text_width: usize, down: bool) {
        let total_lines = self.rope.len_lines();
        let new_positions: Vec<usize> = self
            .cursors
            .iter()
            .map(|&cursor| {
                let line = self.rope.char_to_line(cursor);
                let line_start = self.rope.line_to_char(line);
                let char_col = cursor - line_start;
                let line_str = self.rope.line(line).to_string();
                let display = line_str.trim_end_matches('\n');
                let rows = crate::ui::text::wrap_display_windows(display, text_width);
                let display_col = display
                    .chars()
                    .take(char_col)
                    .map(char_display_width)
                    .sum::<usize>();
                let row_idx = rows
                    .iter()
                    .position(|row| display_col < row.start_col + text_width)
                    .unwrap_or(rows.len() - 1);
                // Column within the row, so the cursor keeps its screen column.
                let col_in_row = display_col.saturating_sub(rows[row_idx].start_col);

                if down {
                    if row_idx + 1 < rows.len() {
                        let target_col = rows[row_idx + 1].start_col + col_in_row;
                        line_start + char_index_at_display_col(display, target_col)
                    } else if line + 1 < total_lines {
                        let next_start = self.rope.line_to_char(line + 1);
                        let next_str = self.rope.line(line + 1).to_string();
                        let next_display = next_str.trim_end_matches('\n');
                        next_start + char_index_at_display_col(next_display, col_in_row)
                    } else {
                        cursor
                    }
                } else if row_idx > 0 {
                    let target_col = rows[row_idx - 1].start_col + col_in_row;
                    line_start + char_index_at_display_col(display, target_col)
                } else if line > 0 {
                    let prev_start = self.rope.line_to_char(line - 1);
                    let prev_str = self.rope.line(line - 1).to_string();
                    let prev_display = prev_str.trim_end_matches('\n');
                    let prev_rows = crate::ui::text::wrap_display_windows(prev_display, text_width);
                    let last = prev_rows[prev_rows.len() - 1];
                    prev_start
                        + char_index_at_display_col(prev_display, last.start_col + col_in_row)
                } else {
                    cursor
                }
            })
            .collect();
        self.cursors = new_positions;
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

    pub(super) fn word_forward_pos(&self, pos: usize, long_word: bool) -> usize {
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

    pub(super) fn move_word_forward_impl(&mut self, long_word: bool) {
        let new_positions: Vec<usize> = self
            .cursors
            .iter()
            .map(|&pos| self.word_forward_pos(pos, long_word))
            .collect();
        self.cursors = new_positions;
        self.sort_and_dedup_cursors();
        self.sync_selection_head();
    }

    pub(super) fn word_forward_end_pos(
        &self,
        pos: usize,
        long_word: bool,
        in_selection: bool,
    ) -> usize {
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

    pub(super) fn move_word_forward_end_impl(&mut self, long_word: bool) {
        let new_positions: Vec<usize> = self
            .cursors
            .iter()
            .enumerate()
            .map(|(i, &pos)| {
                let in_selection = self.selections[i].is_some();
                self.word_forward_end_pos(pos, long_word, in_selection)
            })
            .collect();
        self.cursors = new_positions;
        self.sort_and_dedup_cursors();
        self.sync_selection_head();
    }

    pub(super) fn word_backward_pos(&self, pos: usize, long_word: bool) -> usize {
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

    pub(super) fn move_word_backward_impl(&mut self, long_word: bool) {
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
}

/// Char index within `display` whose cell covers display column `col`, clamped
/// to one past the last character when the column falls past the line end.
fn char_index_at_display_col(display: &str, col: usize) -> usize {
    let mut acc = 0usize;
    for (idx, ch) in display.chars().enumerate() {
        let width = char_display_width(ch);
        if acc + width > col {
            return idx;
        }
        acc += width;
    }
    display.chars().count()
}
