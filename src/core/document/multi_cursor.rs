use super::*;

impl Document {
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
    pub(super) fn sort_and_dedup_cursors(&mut self) {
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
}
