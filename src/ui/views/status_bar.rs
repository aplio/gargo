use crate::core::mode::Mode;
use crate::ui::framework::cell::CellStyle;
use crate::ui::framework::component::{Component, RenderContext};
use crate::ui::framework::surface::Surface;
use crate::ui::text::{display_width, truncate_to_width};

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
            "{}{}{}{}{}{}",
            mode_indicator, recording_indicator, ctx.chord_display, filename, modified, sel_info
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

        let left_w = display_width(&left);
        let right_w = display_width(&right);
        let content_w = left_w + right_w;
        let padding = if cols > content_w {
            cols - content_w
        } else {
            1
        };

        let bar = format!("{}{}{}", left, " ".repeat(padding), right);
        let (bar_truncated, _) = truncate_to_width(&bar, cols);

        let reverse_style = CellStyle {
            reverse: true,
            ..CellStyle::default()
        };

        surface.put_str(0, status_row, bar_truncated, &reverse_style);
    }
}
