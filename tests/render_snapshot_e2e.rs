use gargo::core::editor::Editor;
use gargo::core::mode::Mode;
use gargo::input::action::WindowSplitAxis;
use gargo::input::chord::KeyState;
use gargo::syntax::theme::Theme;
use gargo::ui::framework::component::RenderContext;
use gargo::ui::framework::compositor::Compositor;
use gargo::ui::overlays::github::issue_picker::{IssueCommentEntry, IssueEntry, IssueListPicker};
use std::path::Path;

mod support;

const COLS: usize = 50;
const ROWS: usize = 8;

#[test]
fn home_screen_frame_matches_fixture() {
    let editor = Editor::new();
    let config = gargo::config::Config::default();
    let theme = Theme::dark();
    let key_state = KeyState::Normal;
    let mut compositor = Compositor::new();
    let mut out = Vec::new();
    let ctx = RenderContext::new(
        COLS,
        ROWS,
        &editor,
        &theme,
        &key_state,
        &config,
        Path::new("/tmp/gargo-test-root"),
        false,
        true,
    );
    compositor
        .render(&ctx, &mut out)
        .expect("render home screen frame");
    let rows = support::render_snapshot::ansi_bytes_to_rows(&out, COLS, ROWS);
    support::render_snapshot::assert_rows_match_fixture("home_screen", &rows);
}

#[test]
fn scratch_baseline_frame_matches_fixture() {
    let editor = Editor::new();
    support::render_snapshot::assert_render_matches_fixture(
        "scratch_baseline",
        &editor,
        COLS,
        ROWS,
    );
}

#[test]
fn basic_multiline_frame_matches_fixture() {
    let mut editor = Editor::new();
    editor
        .active_buffer_mut()
        .insert_text("alpha line\nbeta line\ngamma line\n");
    editor.active_buffer_mut().set_cursor_line_char(1, 2);
    support::render_snapshot::assert_render_matches_fixture("basic_multiline", &editor, COLS, ROWS);
}

#[test]
fn insert_mode_frame_matches_fixture() {
    let mut editor = Editor::new();
    editor
        .active_buffer_mut()
        .insert_text("insert mode smoke\n");
    editor.active_buffer_mut().set_cursor_line_char(0, 6);
    editor.mode = Mode::Insert;
    support::render_snapshot::assert_render_matches_fixture("insert_mode", &editor, COLS, ROWS);
}

#[test]
fn scrolled_viewport_frame_matches_fixture() {
    let mut editor = Editor::new();
    let content: String = (0..20).map(|i| format!("line {i}\n")).collect();
    editor.active_buffer_mut().insert_text(&content);
    // view_height = ROWS - 2 = 6; scroll down by 5
    editor.active_buffer_mut().scroll_viewport(5, ROWS - 2);
    support::render_snapshot::assert_render_matches_fixture(
        "scrolled_viewport",
        &editor,
        COLS,
        ROWS,
    );
}

#[test]
fn vertical_split_frame_matches_fixture() {
    let mut editor = Editor::new();
    editor.active_buffer_mut().insert_text("left pane\n");
    let right_id = editor.new_buffer();
    editor.active_buffer_mut().insert_text("right pane\n");
    editor.mode = Mode::Insert;

    support::render_snapshot::assert_render_with_compositor_matches_fixture(
        "vertical_split",
        &editor,
        COLS,
        ROWS,
        move |compositor, cols, rows| {
            compositor.set_focused_buffer(1);
            compositor
                .split_focused_window(WindowSplitAxis::Vertical, right_id, cols, rows)
                .expect("split succeeds");
        },
    );
}

#[test]
fn horizontal_split_frame_matches_fixture() {
    let mut editor = Editor::new();
    editor.active_buffer_mut().insert_text("top pane\n");
    let bottom_id = editor.new_buffer();
    editor.active_buffer_mut().insert_text("bottom pane\n");
    editor.mode = Mode::Insert;

    support::render_snapshot::assert_render_with_compositor_matches_fixture(
        "horizontal_split",
        &editor,
        COLS,
        ROWS,
        move |compositor, cols, rows| {
            compositor.set_focused_buffer(1);
            compositor
                .split_focused_window(WindowSplitAxis::Horizontal, bottom_id, cols, rows)
                .expect("split succeeds");
        },
    );
}

#[test]
fn markdown_link_hover_overlay_matches_fixture_no_border() {
    let mut editor = Editor::new();
    editor.active_buffer_mut().insert_text("hover snapshot\n");
    editor.mode = Mode::Insert;

    support::render_snapshot::assert_render_with_compositor_matches_fixture(
        "markdown_link_hover_no_border",
        &editor,
        COLS,
        ROWS,
        move |compositor, _, _| {
            compositor.set_markdown_link_hover_candidates(vec![
                "alpha.md".to_string(),
                "beta.md".to_string(),
                "gamma.md".to_string(),
            ]);
        },
    );
}

#[test]
fn issue_list_overlay_matches_fixture() {
    let editor = Editor::new();
    let issue_entry = IssueEntry {
        number: 77,
        title: "Issue viewer preview".to_string(),
        body: "Issue body details.".to_string(),
        url: "https://github.com/user/repo/issues/77".to_string(),
        state: "OPEN".to_string(),
        author: "alice".to_string(),
        created_at: "2026-02-01T10:00:00Z".to_string(),
        labels: vec!["bug".to_string(), "ui".to_string()],
        comments: vec![
            IssueCommentEntry {
                author: "bob".to_string(),
                body: "First comment line".to_string(),
                created_at: "2026-02-02T08:00:00Z".to_string(),
            },
            IssueCommentEntry {
                author: "charlie".to_string(),
                body: "Second comment".to_string(),
                created_at: "2026-02-03T12:00:00Z".to_string(),
            },
        ],
        comment_count: 2,
    };
    support::render_snapshot::assert_render_with_compositor_matches_fixture(
        "issue_list_overlay",
        &editor,
        100,
        20,
        move |compositor, _, _| {
            compositor.open_issue_list_picker(IssueListPicker::new(vec![issue_entry]));
        },
    );
}
