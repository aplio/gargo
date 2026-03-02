use super::*;

impl Document {
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

    pub(super) fn sync_selection_head(&mut self) {
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
}
