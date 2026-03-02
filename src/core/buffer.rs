/// Re-export DocumentId as BufferId for backwards compatibility.
pub type BufferId = super::document::DocumentId;

/// Describes a single edit for tree-sitter incremental parsing.
#[derive(Debug, Clone)]
pub struct EditEvent {
    pub start_byte: usize,
    pub old_end_byte: usize,
    pub new_end_byte: usize,
    /// (row, column_byte)
    pub start_position: (usize, usize),
    /// (row, column_byte)
    pub old_end_position: (usize, usize),
    /// (row, column_byte)
    pub new_end_position: (usize, usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CharClass {
    Word,
    Whitespace,
    Other,
}

pub fn char_class(c: char) -> CharClass {
    if c.is_alphanumeric() || c == '_' {
        CharClass::Word
    } else if c.is_whitespace() {
        CharClass::Whitespace
    } else {
        CharClass::Other
    }
}
