#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LspSeverity {
    Error,
    Warning,
    Info,
    Hint,
}

impl LspSeverity {
    pub fn from_lsp_code(code: Option<u64>) -> Self {
        match code {
            Some(1) => Self::Error,
            Some(2) => Self::Warning,
            Some(3) => Self::Info,
            Some(4) => Self::Hint,
            _ => Self::Warning,
        }
    }

    pub fn rank(self) -> u8 {
        match self {
            Self::Error => 4,
            Self::Warning => 3,
            Self::Info => 2,
            Self::Hint => 1,
        }
    }
}

#[derive(Debug, Clone)]
pub struct LspDiagnostic {
    pub range_start_line: usize,
    pub range_start_character_utf16: usize,
    pub message: String,
    pub severity: LspSeverity,
    pub source: Option<String>,
}

#[derive(Debug, Clone)]
pub struct LspLocation {
    pub uri: String,
    pub line: usize,
    pub character_utf16: usize,
}
