use crossterm::style::Color;

use crate::ui::framework::cell::CellStyle;
use crate::ui::framework::component::{Component, RenderContext};
use crate::ui::framework::surface::Surface;
use crate::ui::text::truncate_to_width;

pub struct NotificationBar;

impl Default for NotificationBar {
    fn default() -> Self {
        Self::new()
    }
}

impl NotificationBar {
    pub fn new() -> Self {
        Self
    }
}

impl Component for NotificationBar {
    fn render(&self, ctx: &RenderContext, surface: &mut Surface) {
        let cols = ctx.cols;
        let rows = ctx.rows;
        let notification_row = rows - 1;

        if let Some(msg) = &ctx.editor.message {
            let style = if ctx.close_confirm_active {
                CellStyle {
                    fg: Some(Color::Red),
                    ..CellStyle::default()
                }
            } else {
                CellStyle::default()
            };
            let (truncated, _) = truncate_to_width(msg, cols);
            surface.put_str(0, notification_row, truncated, &style);
        } else if let Some(msg) = ctx.editor.active_line_diagnostic_message() {
            let (truncated, _) = truncate_to_width(msg, cols);
            surface.put_str(0, notification_row, truncated, &CellStyle::default());
        } else {
            let style = CellStyle::default();
            // Clear the row when no message
            surface.fill_region(0, notification_row, cols, ' ', &style);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::core::editor::Editor;
    use crate::input::chord::KeyState;
    use crate::syntax::theme::Theme;
    use crate::ui::framework::component::RenderContext;

    fn render_first_cell_color(close_confirm_active: bool) -> Option<Color> {
        let mut editor = Editor::new();
        editor.message = Some("warning".to_string());
        let config = Config::default();
        let theme = Theme::dark();
        let key_state = KeyState::Normal;
        let ctx = RenderContext::new(
            40,
            4,
            &editor,
            &theme,
            &key_state,
            &config,
            std::path::Path::new("/tmp/gargo-test-root"),
            close_confirm_active,
            false,
        );
        let mut surface = Surface::new(40, 4);

        NotificationBar::new().render(&ctx, &mut surface);

        surface.get(0, 3).style.fg
    }

    #[test]
    fn close_confirm_message_is_red() {
        assert_eq!(render_first_cell_color(true), Some(Color::Red));
    }

    #[test]
    fn normal_message_uses_default_color() {
        assert_eq!(render_first_cell_color(false), None);
    }
}
