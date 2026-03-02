use crossterm::style::Color;
use gargo::config::Config;
use gargo::core::editor::Editor;
use gargo::input::chord::KeyState;
use gargo::syntax::theme::Theme;
use gargo::ui::framework::component::RenderContext;
use gargo::ui::framework::surface::Surface;
use gargo::ui::framework::window_manager::PaneRect;
use gargo::ui::views::text_view::TextView;
use std::path::Path;

fn find_char_in_row(surface: &Surface, row: usize, ch: char) -> usize {
    (0..surface.width)
        .find(|&x| surface.get(x, row).symbol == ch.to_string())
        .expect("expected char in row")
}

#[test]
fn in_editor_diff_overlay_colors_added_removed_and_hunk_lines() {
    let mut editor = Editor::new();
    editor.active_buffer_mut().insert_text(
        "IN-EDITOR DIFF VIEW\n\
Project: /tmp/repo\n\
\n\
## Changed (unstaged)\n\
diff --git a/a.txt b/a.txt\n\
@@ -1 +1 @@\n\
-old\n\
+new\n",
    );

    let config = Config {
        show_line_number: false,
        ..Config::default()
    };
    let theme = Theme::dark();
    let key_state = KeyState::Normal;
    let ctx = RenderContext::new(
        50,
        14,
        &editor,
        &theme,
        &key_state,
        &config,
        Path::new("/tmp/repo"),
        false,
        false,
    );
    let mut surface = Surface::new(50, 14);
    TextView::new().render_buffer(
        &ctx,
        &mut surface,
        editor.active_buffer(),
        PaneRect {
            x: 0,
            y: 0,
            width: 50,
            height: 14,
        },
        false,
        false,
    );

    let hunk_x = find_char_in_row(&surface, 5, '@');
    assert_eq!(surface.get(hunk_x, 5).style.fg, Some(Color::Yellow));

    let minus_x = find_char_in_row(&surface, 6, '-');
    assert_eq!(surface.get(minus_x, 6).style.fg, Some(Color::Red));

    let plus_x = find_char_in_row(&surface, 7, '+');
    assert_eq!(surface.get(plus_x, 7).style.fg, Some(Color::Green));
}
