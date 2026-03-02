use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use gargo::core::mode::Mode;
use gargo::input::action::{Action, CoreAction};
use gargo::input::chord::KeyState;
use gargo::input::keymap::resolve;

fn ctrl_arrow(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::CONTROL)
}

fn shift_arrow(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::SHIFT)
}

fn ctrl_shift_arrow(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::CONTROL | KeyModifiers::SHIFT)
}

#[test]
fn ctrl_left_right_move_word_without_selection_in_insert_and_normal_modes() {
    for mode in [Mode::Insert, Mode::Normal] {
        let mut state = KeyState::Normal;
        let left = resolve(ctrl_arrow(KeyCode::Left), &mut state, &mode, false);
        assert_eq!(left, Action::Core(CoreAction::MoveWordBackwardNoSelect));

        let right = resolve(ctrl_arrow(KeyCode::Right), &mut state, &mode, false);
        assert_eq!(right, Action::Core(CoreAction::MoveWordForwardNoSelect));
    }
}

#[test]
fn ctrl_up_down_alias_vertical_cursor_movement_in_all_modes() {
    for mode in [Mode::Insert, Mode::Normal, Mode::Visual] {
        let mut state = KeyState::Normal;
        let up = resolve(ctrl_arrow(KeyCode::Up), &mut state, &mode, false);
        assert_eq!(up, Action::Core(CoreAction::MoveUp));

        let down = resolve(ctrl_arrow(KeyCode::Down), &mut state, &mode, false);
        assert_eq!(down, Action::Core(CoreAction::MoveDown));
    }
}

#[test]
fn visual_mode_ctrl_right_remains_noop() {
    let mut state = KeyState::Normal;
    let action = resolve(ctrl_arrow(KeyCode::Right), &mut state, &Mode::Visual, false);
    assert_eq!(action, Action::Core(CoreAction::Noop));
}

#[test]
fn shift_left_right_extend_char_selection_in_normal_and_visual_modes() {
    for mode in [Mode::Normal, Mode::Visual] {
        let mut state = KeyState::Normal;
        let left = resolve(shift_arrow(KeyCode::Left), &mut state, &mode, false);
        assert_eq!(left, Action::Core(CoreAction::ExtendLeft));

        let right = resolve(shift_arrow(KeyCode::Right), &mut state, &mode, false);
        assert_eq!(right, Action::Core(CoreAction::ExtendRight));
    }
}

#[test]
fn ctrl_shift_left_right_extend_word_selection_in_normal_and_visual_modes() {
    for mode in [Mode::Normal, Mode::Visual] {
        let mut state = KeyState::Normal;
        let left = resolve(ctrl_shift_arrow(KeyCode::Left), &mut state, &mode, false);
        assert_eq!(left, Action::Core(CoreAction::ExtendWordBackwardShift));

        let right = resolve(ctrl_shift_arrow(KeyCode::Right), &mut state, &mode, false);
        assert_eq!(right, Action::Core(CoreAction::ExtendWordForwardShift));
    }
}
