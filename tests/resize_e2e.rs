use gargo::config::Config;
use gargo::core::editor::Editor;
use gargo::input::chord::KeyState;
use gargo::syntax::theme::Theme;
use gargo::ui::framework::component::RenderContext;
use gargo::ui::framework::compositor::Compositor;
use std::path::Path;

mod support;

const LARGE_COLS: usize = 50;
const LARGE_ROWS: usize = 10;
const SMALL_COLS: usize = 30;
const SMALL_ROWS: usize = 6;

fn make_editor() -> Editor {
    let mut editor = Editor::new();
    editor
        .active_buffer_mut()
        .insert_text("alpha line\nbeta line\ngamma line\n");
    editor.active_buffer_mut().set_cursor_line_char(0, 0);
    editor
}

/// After shrinking the terminal, the rendered frame must match a clean render
/// at the new size — no stale content from the old larger layout.
#[test]
fn resize_shrink_produces_clean_frame() {
    let editor = make_editor();
    let config = Config::default();
    let theme = Theme::dark();
    let key_state = KeyState::Normal;
    let project_root = Path::new("/tmp/gargo-test-root");

    let mut compositor = Compositor::new();

    // First render at large size (establishes internal surface state)
    let mut out1 = Vec::new();
    let ctx1 = RenderContext::new(
        LARGE_COLS,
        LARGE_ROWS,
        &editor,
        &theme,
        &key_state,
        &config,
        project_root,
        false,
        false,
    );
    compositor.render(&ctx1, &mut out1).expect("render large");

    // Second render at small size (simulates resize)
    let mut out2 = Vec::new();
    let ctx2 = RenderContext::new(
        SMALL_COLS,
        SMALL_ROWS,
        &editor,
        &theme,
        &key_state,
        &config,
        project_root,
        false,
        false,
    );
    compositor.render(&ctx2, &mut out2).expect("render small");

    let actual = support::render_snapshot::ansi_bytes_to_rows(&out2, SMALL_COLS, SMALL_ROWS);
    support::render_snapshot::assert_rows_match_fixture("resize_shrink", &actual);
}

/// After growing the terminal, newly exposed rows/columns must be properly
/// painted — not blank or garbled.
#[test]
fn resize_grow_produces_clean_frame() {
    let editor = make_editor();
    let config = Config::default();
    let theme = Theme::dark();
    let key_state = KeyState::Normal;
    let project_root = Path::new("/tmp/gargo-test-root");

    let mut compositor = Compositor::new();

    // First render at small size
    let mut out1 = Vec::new();
    let ctx1 = RenderContext::new(
        SMALL_COLS,
        SMALL_ROWS,
        &editor,
        &theme,
        &key_state,
        &config,
        project_root,
        false,
        false,
    );
    compositor.render(&ctx1, &mut out1).expect("render small");

    // Second render at large size (simulates grow)
    let mut out2 = Vec::new();
    let ctx2 = RenderContext::new(
        LARGE_COLS,
        LARGE_ROWS,
        &editor,
        &theme,
        &key_state,
        &config,
        project_root,
        false,
        false,
    );
    compositor.render(&ctx2, &mut out2).expect("render large");

    let actual = support::render_snapshot::ansi_bytes_to_rows(&out2, LARGE_COLS, LARGE_ROWS);
    support::render_snapshot::assert_rows_match_fixture("resize_grow", &actual);
}

/// Core bug reproduction: simulates a terminal that has content from a previous
/// large render, then applies the output from a smaller resize render on top.
/// Without the fix, stale content from the old layout bleeds through in blank areas.
#[test]
fn resize_clears_stale_content() {
    let editor = make_editor();
    let config = Config::default();
    let theme = Theme::dark();
    let key_state = KeyState::Normal;
    let project_root = Path::new("/tmp/gargo-test-root");

    let mut compositor = Compositor::new();

    // Render at large size — this is the "terminal screen" before resize
    let mut out1 = Vec::new();
    let ctx1 = RenderContext::new(
        LARGE_COLS,
        LARGE_ROWS,
        &editor,
        &theme,
        &key_state,
        &config,
        project_root,
        false,
        false,
    );
    compositor.render(&ctx1, &mut out1).expect("render large");

    // Build simulated terminal screen from the large render
    let mut screen = vec![vec![' '; LARGE_COLS]; LARGE_ROWS];
    support::render_snapshot::apply_ansi_to_screen(&mut screen, &out1, LARGE_COLS, LARGE_ROWS);

    // Now "resize" to smaller — render with the same compositor
    let mut out2 = Vec::new();
    let ctx2 = RenderContext::new(
        SMALL_COLS,
        SMALL_ROWS,
        &editor,
        &theme,
        &key_state,
        &config,
        project_root,
        false,
        false,
    );
    compositor.render(&ctx2, &mut out2).expect("render small");

    // Apply the resize render output on top of the old screen
    // (simulating what the real terminal displays)
    let actual =
        support::render_snapshot::apply_ansi_to_screen(&mut screen, &out2, SMALL_COLS, SMALL_ROWS);

    // Reference: what a clean render at the small size should look like
    let mut fresh_compositor = Compositor::new();
    let mut out_ref = Vec::new();
    fresh_compositor
        .render(&ctx2, &mut out_ref)
        .expect("render reference");
    let expected = support::render_snapshot::ansi_bytes_to_rows(&out_ref, SMALL_COLS, SMALL_ROWS);

    assert_eq!(
        actual, expected,
        "After resize, terminal content must match a clean render.\n\
         Stale content from the old layout is bleeding through.\n\
         actual (with stale): {:?}\n\
         expected (clean):    {:?}",
        actual, expected,
    );
}
