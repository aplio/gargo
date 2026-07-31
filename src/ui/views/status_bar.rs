use crossterm::style::Color;

use crate::core::mode::Mode;
use crate::syntax::ui_colors::UiColors;
use crate::ui::framework::cell::CellStyle;
use crate::ui::framework::component::{Component, RenderContext};
use crate::ui::framework::surface::Surface;
use crate::ui::text::{display_width, truncate_to_width};

fn mode_bg(ui: &UiColors, mode: Mode) -> Color {
    match mode {
        Mode::Normal => ui.mode_normal,
        Mode::Insert => ui.mode_insert,
        Mode::Visual => ui.mode_visual,
    }
}

pub struct StatusBar;

impl Default for StatusBar {
    fn default() -> Self {
        Self::new()
    }
}

impl StatusBar {
    pub fn new() -> Self {
        Self
    }
}

impl Component for StatusBar {
    fn render(&self, ctx: &RenderContext, surface: &mut Surface) {
        let cols = ctx.cols;
        let rows = ctx.rows;
        let status_row = rows - 2;

        let mode_indicator = format!(" {} ", ctx.editor.mode.short_name());

        let recording_indicator = if let Some(reg) = ctx.editor.macro_recorder.recording_register()
        {
            format!("[recording @{}] ", reg)
        } else {
            String::new()
        };

        let buf = ctx.editor.active_buffer();
        let filename = buf.status_bar_path();
        let modified = if buf.dirty { " [+]" } else { "" };

        let sel_info = if ctx.editor.mode == Mode::Visual {
            if let Some((start, end)) = buf.selection_range() {
                format!(" [sel: {} chars]", end - start)
            } else {
                String::new()
            }
        } else {
            String::new()
        };

        let left = format!(
            "{}{}{}{}{}",
            recording_indicator, ctx.chord_display, filename, modified, sel_info
        );

        let lang_name = ctx.editor.active_language_name().unwrap_or("");
        let lang_indicator = if lang_name.is_empty() {
            String::new()
        } else {
            format!("{} ", lang_name)
        };
        let buf_info = format!(
            "[{}/{}] ",
            ctx.editor.active_index() + 1,
            ctx.editor.buffer_count()
        );
        let right = format!(
            "{}{}{}:{}",
            lang_indicator,
            buf_info,
            buf.display_cursor_line() + 1,
            buf.display_cursor_col() + 1
        );

        let ui = &ctx.theme.ui;
        let bar_style = CellStyle {
            fg: Some(ui.status_fg),
            bg: Some(ui.status_bg),
            ..CellStyle::default()
        };
        let mode_style = CellStyle {
            fg: Some(ui.mode_fg),
            bg: Some(mode_bg(ui, ctx.editor.mode)),
            bold: true,
            ..CellStyle::default()
        };

        // Paint the whole row first so trailing cells carry the bar background
        // even when the text is shorter than the terminal width.
        surface.fill_region(0, status_row, cols, ' ', &bar_style);

        let mode_w = display_width(&mode_indicator).min(cols);
        let (mode_truncated, _) = truncate_to_width(&mode_indicator, cols);
        surface.put_str(0, status_row, mode_truncated, &mode_style);

        let rest_cols = cols.saturating_sub(mode_w);
        if rest_cols == 0 {
            return;
        }

        let left_w = display_width(&left);
        let right_w = display_width(&right);
        let content_w = left_w + right_w;
        let padding = if rest_cols > content_w {
            rest_cols - content_w
        } else {
            1
        };

        let bar = format!("{}{}{}", left, " ".repeat(padding), right);
        let (bar_truncated, _) = truncate_to_width(&bar, rest_cols);

        surface.put_str(mode_w, status_row, bar_truncated, &bar_style);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::core::editor::Editor;
    use crate::input::chord::KeyState;
    use crate::syntax::theme::Theme;

    fn render(mode: Mode) -> Surface {
        render_with_theme(mode, Theme::dark())
    }

    fn render_with_theme(mode: Mode, theme: Theme) -> Surface {
        let mut editor = Editor::new();
        editor.mode = mode;
        let config = Config::default();
        let key_state = KeyState::Normal;
        let ctx = RenderContext::new(
            40,
            4,
            &editor,
            &theme,
            &key_state,
            &config,
            std::path::Path::new("/tmp/gargo-test-root"),
            false,
            false,
        );
        let mut surface = Surface::new(40, 4);
        StatusBar::new().render(&ctx, &mut surface);
        surface
    }

    #[test]
    fn bar_uses_explicit_colors_not_reverse() {
        let surface = render(Mode::Normal);
        for x in 0..40 {
            let style = &surface.get(x, 2).style;
            assert!(!style.reverse, "cell {} still uses reverse", x);
            assert!(style.bg.is_some(), "cell {} has no background", x);
        }
    }

    #[test]
    fn trailing_cells_keep_bar_background() {
        let surface = render(Mode::Normal);
        assert_eq!(
            surface.get(39, 2).style.bg,
            Some(UiColors::dark().status_bg)
        );
    }

    #[test]
    fn mode_indicator_is_colored_per_mode() {
        let ui = UiColors::dark();
        for (mode, expected) in [
            (Mode::Normal, ui.mode_normal),
            (Mode::Insert, ui.mode_insert),
            (Mode::Visual, ui.mode_visual),
        ] {
            let surface = render(mode);
            let style = &surface.get(1, 2).style;
            assert_eq!(style.bg, Some(expected), "mode {:?}", mode);
            assert!(style.bold);
            // Text after the mode segment falls back to the bar colors.
            assert_eq!(surface.get(6, 2).style.bg, Some(ui.status_bg));
        }
    }

    #[test]
    fn bar_follows_the_theme_preset() {
        let surface = render_with_theme(Mode::Normal, Theme::gargo_light());
        assert_eq!(
            surface.get(39, 2).style.bg,
            Some(UiColors::light().status_bg)
        );
    }
}
