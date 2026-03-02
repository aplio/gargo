use unicode_width::UnicodeWidthStr;

use crate::core_lib::ui::cell::Cell;
use crate::core_lib::ui::style::CellStyle;
use crate::core_lib::ui::text::char_display_width;

pub struct Surface {
    cells: Vec<Cell>,
    pub width: usize,
    pub height: usize,
}

impl Surface {
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            cells: vec![Cell::default(); width * height],
            width,
            height,
        }
    }

    pub fn resize(&mut self, width: usize, height: usize) {
        self.width = width;
        self.height = height;
        self.cells.resize(width * height, Cell::default());
        self.reset();
    }

    pub fn reset(&mut self) {
        for cell in &mut self.cells {
            cell.reset();
        }
    }

    #[inline]
    fn index(&self, x: usize, y: usize) -> usize {
        y * self.width + x
    }

    pub fn get(&self, x: usize, y: usize) -> &Cell {
        &self.cells[self.index(x, y)]
    }

    pub fn get_mut(&mut self, x: usize, y: usize) -> &mut Cell {
        let idx = self.index(x, y);
        &mut self.cells[idx]
    }

    /// Fix up wide character boundaries before overwriting cell at (col, y).
    /// Must be called BEFORE writing new content to the cell.
    ///
    /// When a cell that is part of a wide character (either the main cell or
    /// its continuation) is about to be overwritten, the other half becomes
    /// invalid. This method clears the orphaned half to a space so that the
    /// double-buffered diff correctly detects the change.
    fn fixup_wide_char(&mut self, col: usize, y: usize) {
        let idx = self.index(col, y);

        // If this cell is a continuation of a wide char (empty symbol),
        // the owning wide char at col-1 will lose its right half.
        if self.cells[idx].symbol.is_empty() && col > 0 {
            let owner_idx = self.index(col - 1, y);
            self.cells[owner_idx].symbol.clear();
            self.cells[owner_idx].symbol.push(' ');
        }

        // If this cell holds a wide char (width 2), its continuation at
        // col+1 will be orphaned.
        if !self.cells[idx].symbol.is_empty() {
            let w = UnicodeWidthStr::width(self.cells[idx].symbol.as_str());
            if w == 2 && col + 1 < self.width {
                let cont_idx = self.index(col + 1, y);
                self.cells[cont_idx].symbol.clear();
                self.cells[cont_idx].symbol.push(' ');
            }
        }
    }

    /// Write a string at (x, y) with the given style.
    /// Returns the number of columns consumed (accounting for CJK width).
    pub fn put_str(&mut self, x: usize, y: usize, s: &str, style: &CellStyle) -> usize {
        if y >= self.height {
            return 0;
        }
        let mut col = x;
        for ch in s.chars() {
            if ch == '\t' {
                let tab_width = char_display_width(ch);
                if col + tab_width > self.width {
                    break;
                }
                for _ in 0..tab_width {
                    self.fixup_wide_char(col, y);
                    let cell = self.get_mut(col, y);
                    cell.symbol.clear();
                    cell.symbol.push(' ');
                    cell.style = *style;
                    col += 1;
                }
                continue;
            }

            let ch_width = char_display_width(ch);
            if ch_width == 0 {
                continue;
            }
            if col + ch_width > self.width {
                break;
            }

            // Fix wide char boundaries before overwriting
            self.fixup_wide_char(col, y);
            if ch_width == 2 && col + 1 < self.width {
                self.fixup_wide_char(col + 1, y);
            }

            // First cell: store the character
            let cell = self.get_mut(col, y);
            cell.symbol.clear();
            cell.symbol.push(ch);
            cell.style = *style;

            // For wide characters, mark continuation cells
            if ch_width == 2 && col + 1 < self.width {
                let cont = self.get_mut(col + 1, y);
                cont.symbol.clear(); // empty string = continuation
                cont.style = *style;
            }

            col += ch_width;
        }
        col - x
    }

    /// Fill a horizontal region starting at (x, y) with character `ch` repeated `w` times.
    pub fn fill_region(&mut self, x: usize, y: usize, w: usize, ch: char, style: &CellStyle) {
        if y >= self.height {
            return;
        }
        let ch_width = char_display_width(ch).max(1);
        let mut col = x;
        let end = (x + w).min(self.width);
        while col + ch_width <= end {
            // Fix wide char boundaries before overwriting
            self.fixup_wide_char(col, y);
            if ch_width == 2 && col + 1 < self.width {
                self.fixup_wide_char(col + 1, y);
            }

            let cell = self.get_mut(col, y);
            cell.symbol.clear();
            cell.symbol.push(ch);
            cell.style = *style;
            if ch_width == 2 && col + 1 < self.width {
                let cont = self.get_mut(col + 1, y);
                cont.symbol.clear();
                cont.style = *style;
            }
            col += ch_width;
        }
    }
}
