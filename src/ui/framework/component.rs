use crossterm::cursor::SetCursorStyle;
use std::path::Path;

use crate::config::Config;
use crate::core::editor::Editor;
use crate::input::chord::KeyState;
use crate::syntax::highlight::HighlightManager;
use crate::syntax::theme::Theme;
use crate::ui::framework::surface::Surface;

pub struct RenderContext<'a> {
    pub cols: usize,
    pub rows: usize,
    pub editor: &'a Editor,
    pub highlight_manager: &'a HighlightManager,
    pub theme: &'a Theme,
    pub chord_display: &'a str,
    pub config: &'a Config,
    pub project_root: &'a Path,
    pub close_confirm_active: bool,
    pub home_screen_active: bool,
    pub editor_area_x: usize,
    pub editor_area_width: usize,
}

impl<'a> RenderContext<'a> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        cols: usize,
        rows: usize,
        editor: &'a Editor,
        theme: &'a Theme,
        key_state: &KeyState,
        config: &'a Config,
        project_root: &'a Path,
        close_confirm_active: bool,
        home_screen_active: bool,
    ) -> Self {
        Self::new_with_chord_display(
            cols,
            rows,
            editor,
            theme,
            key_state.display_prefix(),
            config,
            project_root,
            close_confirm_active,
            home_screen_active,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new_with_chord_display(
        cols: usize,
        rows: usize,
        editor: &'a Editor,
        theme: &'a Theme,
        chord_display: &'a str,
        config: &'a Config,
        project_root: &'a Path,
        close_confirm_active: bool,
        home_screen_active: bool,
    ) -> Self {
        Self {
            cols,
            rows,
            editor,
            highlight_manager: &editor.highlight_manager,
            theme,
            chord_display,
            config,
            project_root,
            close_confirm_active,
            home_screen_active,
            editor_area_x: 0,
            editor_area_width: cols,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum EventResult {
    /// Event was consumed by this component.
    Consumed,
    /// Event was not handled; pass to next layer.
    Ignored,
    /// Component emits an action.
    Action(crate::input::action::Action),
}

pub trait Component {
    fn render(&self, ctx: &RenderContext, surface: &mut Surface);

    /// Return cursor position and style, or None to hide cursor.
    fn cursor(&self, _ctx: &RenderContext) -> Option<(u16, u16, SetCursorStyle)> {
        None
    }
}
