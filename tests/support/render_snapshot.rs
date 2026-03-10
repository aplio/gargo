use std::fs;
use std::path::PathBuf;

use gargo::config::Config;
use gargo::core::editor::Editor;
use gargo::input::chord::KeyState;
use gargo::syntax::theme::Theme;
use gargo::ui::framework::component::RenderContext;
use gargo::ui::framework::compositor::Compositor;

const UPDATE_ENV: &str = "UPDATE_RENDER_FIXTURES";

#[allow(dead_code)]
pub fn assert_render_matches_fixture(name: &str, editor: &Editor, cols: usize, rows: usize) {
    let actual_rows = render_rows_with_compositor(editor, cols, rows, |_, _, _| {});
    assert_rows_match_fixture(name, &actual_rows);
}

/// Compare pre-rendered rows against a named fixture file.
/// Supports UPDATE_RENDER_FIXTURES=1 to auto-generate fixtures.
pub fn assert_rows_match_fixture(name: &str, actual_rows: &[String]) {
    let actual = format!("{}\n", actual_rows.join("\n"));
    let fixture = fixture_path(name);

    if std::env::var_os(UPDATE_ENV).is_some() {
        if let Some(parent) = fixture.parent() {
            fs::create_dir_all(parent).expect("create render fixture directory");
        }
        fs::write(&fixture, actual).expect("write render fixture");
        return;
    }

    let expected = fs::read_to_string(&fixture).unwrap_or_else(|_| {
        panic!(
            "Missing fixture: {} (set {}=1 to generate)",
            fixture.display(),
            UPDATE_ENV
        )
    });

    let expected = expected.replace("\r\n", "\n");
    // Normalize version strings so fixtures don't break on version bumps.
    let version_re = regex::Regex::new(r"gargo v\d+\.\d+\.\d+").unwrap();
    let actual_normalized = version_re.replace_all(&actual, "gargo vX.Y.Z");
    let expected_normalized = version_re.replace_all(&expected, "gargo vX.Y.Z");
    assert_eq!(
        actual_normalized, expected_normalized,
        "Render snapshot mismatch for {}. Re-run with {}=1 to update fixtures.",
        name, UPDATE_ENV
    );
}

#[allow(dead_code)]
pub fn assert_render_with_compositor_matches_fixture<F>(
    name: &str,
    editor: &Editor,
    cols: usize,
    rows: usize,
    configure: F,
) where
    F: FnOnce(&mut Compositor, usize, usize),
{
    let actual_rows = render_rows_with_compositor(editor, cols, rows, configure);
    assert_rows_match_fixture(name, &actual_rows);
}

#[allow(dead_code)]
fn render_rows_with_compositor<F>(
    editor: &Editor,
    cols: usize,
    rows: usize,
    configure: F,
) -> Vec<String>
where
    F: FnOnce(&mut Compositor, usize, usize),
{
    let config = Config::default();
    let theme = Theme::dark();
    let key_state = KeyState::Normal;
    let mut compositor = Compositor::new();
    configure(&mut compositor, cols, rows);
    let mut out = Vec::new();

    let ctx = RenderContext::new(
        cols,
        rows,
        editor,
        &theme,
        &key_state,
        &config,
        std::path::Path::new("/tmp/gargo-test-root"),
        false,
        false,
    );
    compositor
        .render(&ctx, &mut out)
        .expect("render frame to memory");

    ansi_bytes_to_rows(&out, cols, rows)
}

/// Parse ANSI byte output into a screen grid of character rows.
/// Starts from a blank screen.
pub fn ansi_bytes_to_rows(bytes: &[u8], cols: usize, rows: usize) -> Vec<String> {
    let mut screen = vec![vec![' '; cols]; rows];
    apply_ansi_bytes_to_screen(&mut screen, bytes, cols, rows);
    screen_to_rows(screen)
}

/// Apply ANSI byte output onto an existing screen grid (simulating a terminal
/// that already has content). This is used to test resize scenarios where the
/// terminal has stale content from a previous render.
#[allow(dead_code)]
pub fn apply_ansi_to_screen(
    screen: &mut Vec<Vec<char>>,
    bytes: &[u8],
    cols: usize,
    rows: usize,
) -> Vec<String> {
    // Truncate or extend screen to match new dimensions
    screen.resize(rows, vec![' '; cols]);
    for row in screen.iter_mut() {
        row.resize(cols, ' ');
    }
    apply_ansi_bytes_to_screen(screen, bytes, cols, rows);
    screen_to_rows(screen.clone())
}

/// Core ANSI parser that writes characters onto a mutable screen grid.
fn apply_ansi_bytes_to_screen(screen: &mut [Vec<char>], bytes: &[u8], cols: usize, rows: usize) {
    let mut cursor_x = 0usize;
    let mut cursor_y = 0usize;
    let mut i = 0usize;

    while i < bytes.len() {
        match bytes[i] {
            b'\x1b' => {
                if i + 1 < bytes.len() && bytes[i + 1] == b'[' {
                    if let Some((next, final_byte, params)) = parse_csi(bytes, i + 2) {
                        if final_byte == b'H' || final_byte == b'f' {
                            let (row, col) = parse_cursor_position(params);
                            cursor_y = row.min(rows.saturating_sub(1));
                            cursor_x = col.min(cols.saturating_sub(1));
                        } else if final_byte == b'J' && params == "2" {
                            // Clear entire screen (\x1b[2J)
                            for row in screen.iter_mut() {
                                for cell in row.iter_mut() {
                                    *cell = ' ';
                                }
                            }
                        }
                        i = next;
                    } else {
                        i += 1;
                    }
                } else {
                    i += 1;
                }
            }
            b'\r' => {
                cursor_x = 0;
                i += 1;
            }
            b'\n' => {
                cursor_y = (cursor_y + 1).min(rows.saturating_sub(1));
                i += 1;
            }
            _ => {
                let s = std::str::from_utf8(&bytes[i..]).expect("valid utf-8 render output");
                let ch = s.chars().next().expect("char exists");
                if cursor_y < rows && cursor_x < cols {
                    screen[cursor_y][cursor_x] = ch;
                }
                cursor_x = (cursor_x + 1).min(cols);
                i += ch.len_utf8();
            }
        }
    }
}

fn screen_to_rows(screen: Vec<Vec<char>>) -> Vec<String> {
    screen
        .into_iter()
        .map(|row| row.into_iter().collect::<String>())
        .map(|row| normalize_row(&row))
        .collect()
}

fn normalize_row(row: &str) -> String {
    row.trim_end_matches(' ').to_string()
}

fn parse_csi(bytes: &[u8], start: usize) -> Option<(usize, u8, &str)> {
    let mut idx = start;
    while idx < bytes.len() {
        let byte = bytes[idx];
        if (0x40..=0x7e).contains(&byte) {
            let params = std::str::from_utf8(&bytes[start..idx]).ok()?;
            return Some((idx + 1, byte, params));
        }
        idx += 1;
    }
    None
}

fn parse_cursor_position(params: &str) -> (usize, usize) {
    let mut parts = params.split(';');
    let row_1_based = parts
        .next()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(1);
    let col_1_based = parts
        .next()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(1);
    (row_1_based.saturating_sub(1), col_1_based.saturating_sub(1))
}

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("render")
        .join(format!("{name}.txt"))
}
