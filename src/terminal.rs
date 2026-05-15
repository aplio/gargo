use std::io::stdout;
use std::panic;

use crossterm::{
    cursor::{self, SetCursorStyle},
    event::{
        DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
    },
    execute,
    terminal::{self, ClearType},
};

/// Enter raw mode, alternate screen, and install panic hook.
pub fn setup() -> std::io::Stdout {
    let mut stdout = stdout();
    terminal::enable_raw_mode().expect("Failed to enable raw mode");
    execute!(
        stdout,
        terminal::EnterAlternateScreen,
        terminal::Clear(ClearType::All),
        EnableBracketedPaste,
        EnableMouseCapture,
        cursor::Show,
    )
    .expect("Failed to setup terminal");

    // Request the Kitty keyboard protocol so terminals that support it
    // (ghostty, kitty, wezterm, foot, alacritty, iTerm2 with the setting on,
    // recent xterm) report Ctrl+digit and other previously-ambiguous chords
    // with their modifier bits intact. Terminals that don't understand the
    // sequence silently ignore it, so this is safe everywhere.
    let _ = execute!(
        stdout,
        PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
    );

    let default_hook = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        let _ = terminal::disable_raw_mode();
        let _ = execute!(
            std::io::stdout(),
            PopKeyboardEnhancementFlags,
            SetCursorStyle::DefaultUserShape,
            DisableBracketedPaste,
            DisableMouseCapture,
            terminal::LeaveAlternateScreen
        );
        default_hook(info);
    }));

    stdout
}

/// Restore terminal to normal state.
pub fn teardown(mut stdout: std::io::Stdout) {
    let _ = terminal::disable_raw_mode();
    let _ = execute!(
        stdout,
        PopKeyboardEnhancementFlags,
        SetCursorStyle::DefaultUserShape,
        DisableBracketedPaste,
        DisableMouseCapture,
        terminal::LeaveAlternateScreen
    );
}
