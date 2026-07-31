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

    /// Number of terminal rows `line_idx` occupies when soft-wrapped at
    /// `text_width` columns.
    pub fn wrapped_line_rows(&self, line_idx: usize, text_width: usize) -> usize {
        if text_width == 0 || line_idx >= self.rope.len_lines() {
            return 1;
        }
        let line = self.rope.line(line_idx).to_string();
        crate::ui::text::wrapped_row_count(line.trim_end_matches('\n'), text_width)
    }

    /// Which wrapped row of its own line the display cursor sits on.
    pub fn wrapped_cursor_row(&self, text_width: usize) -> usize {
        if text_width == 0 {
            return 0;
        }
        let line = self.display_cursor_line();
        let line_str = self.rope.line(line).to_string();
        crate::ui::text::wrapped_row_of_col(
            line_str.trim_end_matches('\n'),
            text_width,
            self.display_cursor_display_col(),
        )
    }

    /// Soft-wrap counterpart of [`Document::ensure_cursor_visible_with_horizontal`].
    /// Scrolls in whole wrapped rows: `scroll_offset` names the top buffer line
    /// and `wrap_scroll_row` how many of its wrapped rows are above the
    /// viewport, so lines taller than the pane can still be scrolled through.
    pub fn ensure_cursor_visible_wrapped(&mut self, view_height: usize, text_width: usize) {
        self.horizontal_scroll_offset = 0;
        if text_width == 0 {
            return;
        }
        let view_height = view_height.max(1);
        let line = self.display_cursor_line();
        let cursor_row = self.wrapped_cursor_row(text_width);

        // Cursor above the viewport top.
        if line < self.scroll_offset {
            self.scroll_offset = line;
            self.wrap_scroll_row = cursor_row;
            return;
        }

        // Cursor inside the top line: scroll within that line only.
        if line == self.scroll_offset {
            if cursor_row < self.wrap_scroll_row {
                self.wrap_scroll_row = cursor_row;
            } else if cursor_row >= self.wrap_scroll_row + view_height {
                self.wrap_scroll_row = cursor_row + 1 - view_height;
            }
            return;
        }

        // Rows from the start of the cursor's line down to the cursor itself.
        let mut used = cursor_row + 1;
        if used > view_height {
            // The cursor line alone overflows the pane: show its tail.
            self.set_wrap_top_if_later(line, used - view_height);
            return;
        }

        // Walk back from the cursor line, taking whole lines while they fit.
        let mut top = line;
        let mut partial_top = None;
        while top > self.scroll_offset {
            let rows = self.wrapped_line_rows(top - 1, text_width);
            if used + rows > view_height {
                // The line above is too tall to show whole — fill the leftover
                // rows with its tail rather than leaving the pane half empty.
                if used < view_height {
                    partial_top = Some((top - 1, rows - (view_height - used)));
                }
                break;
            }
            used += rows;
            top -= 1;
        }
        match partial_top {
            Some((partial_line, partial_row)) => {
                self.set_wrap_top_if_later(partial_line, partial_row)
            }
            None => self.set_wrap_top_if_later(top, 0),
        }
    }

    /// Move the viewport top to `(line, row)` only if that is further down than
    /// where it already sits — this path only ever scrolls forward, and the
    /// cursor is known to be visible from the current top otherwise.
    fn set_wrap_top_if_later(&mut self, line: usize, row: usize) {
        if (line, row) > (self.scroll_offset, self.wrap_scroll_row) {
            self.scroll_offset = line;
            self.wrap_scroll_row = row;
        }
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

    /// Soft-wrap counterpart of [`Document::scroll_viewport`]: `delta` counts
    /// wrapped rows, so the wheel moves the same visual distance whether or not
    /// the lines under it are wrapped.
    pub fn scroll_viewport_wrapped(&mut self, delta: isize, view_height: usize, text_width: usize) {
        if text_width == 0 {
            self.scroll_viewport(delta, view_height);
            return;
        }
        let total_lines = self.rope.len_lines();
        let mut line = self.scroll_offset.min(total_lines.saturating_sub(1));
        let mut sub = self
            .wrap_scroll_row
            .min(self.wrapped_line_rows(line, text_width).saturating_sub(1));

        let mut remaining = delta.unsigned_abs();
        while remaining > 0 {
            if delta >= 0 {
                if sub + 1 < self.wrapped_line_rows(line, text_width) {
                    sub += 1;
                } else if line + 1 < total_lines {
                    line += 1;
                    sub = 0;
                } else {
                    break;
                }
            } else if sub > 0 {
                sub -= 1;
            } else if line > 0 {
                line -= 1;
                sub = self.wrapped_line_rows(line, text_width).saturating_sub(1);
            } else {
                break;
            }
            remaining -= 1;
        }
        self.scroll_offset = line;
        self.wrap_scroll_row = sub;

        // Clamp the primary cursor into the visible range, mirroring
        // `scroll_viewport`.
        let last_visible = self.last_visible_line(view_height, text_width);
        let cursor_line = self.cursor_line();
        let target_line = if cursor_line < line {
            Some(line)
        } else if cursor_line > last_visible {
            Some(last_visible)
        } else {
            None
        };
        if let Some(target_line) = target_line {
            let old_line_start = self.rope.line_to_char(cursor_line);
            let col = self.cursors[0] - old_line_start;
            let line_start = self.rope.line_to_char(target_line);
            let line_len = self.line_len(target_line);
            self.cursors[0] = line_start + col.min(line_len);
        }
    }

    /// Last buffer line with at least one wrapped row inside a `view_height`
    /// pane starting at the current scroll position.
    pub fn last_visible_line(&self, view_height: usize, text_width: usize) -> usize {
        let total_lines = self.rope.len_lines();
        let mut line = self.scroll_offset.min(total_lines.saturating_sub(1));
        let first_line_rows = self
            .wrapped_line_rows(line, text_width)
            .saturating_sub(self.wrap_scroll_row);
        let mut used = first_line_rows.max(1);
        while used < view_height.max(1) && line + 1 < total_lines {
            line += 1;
            used += self.wrapped_line_rows(line, text_width);
        }
        line
    }
}
