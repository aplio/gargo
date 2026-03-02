use crate::core_lib::ui::style::CellStyle;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cell {
    pub symbol: String,
    pub style: CellStyle,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            symbol: " ".to_string(),
            style: CellStyle::default(),
        }
    }
}

impl Cell {
    pub fn reset(&mut self) {
        self.symbol.clear();
        self.symbol.push(' ');
        self.style = CellStyle::default();
    }
}
