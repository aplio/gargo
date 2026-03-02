use super::*;

impl Document {
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
}
