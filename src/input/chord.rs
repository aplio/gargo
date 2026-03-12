#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyState {
    Normal,
    CtrlX,
    Space,
    SpaceWindow,
    Goto,
    MacroRecord,
    MacroPlay,
}

impl KeyState {
    pub fn display_prefix(&self) -> &'static str {
        match self {
            KeyState::CtrlX => "C-x ",
            KeyState::Space => "SPC ",
            KeyState::SpaceWindow => "SPC w ",
            KeyState::Goto => "g ",
            KeyState::MacroRecord => "q ",
            KeyState::MacroPlay => "@ ",
            KeyState::Normal => "",
        }
    }
}
