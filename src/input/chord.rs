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
