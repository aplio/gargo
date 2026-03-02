use std::io::Write;

use gargo::config::Config;
use gargo::core::editor::Editor;
use gargo::syntax::theme::Theme;
use gargo::ui::framework::compositor::Compositor;

// ---------------------------------------------------------------------------
// NullWriter – impl Write that discards all output
// ---------------------------------------------------------------------------

#[allow(dead_code)]
pub struct NullWriter;

impl Write for NullWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Setup helpers
// ---------------------------------------------------------------------------

/// Create an editor loaded with `source`, highlighted as `ext` (e.g. "file.rs", "file.md").
#[allow(dead_code)]
pub fn setup_editor(source: &str, ext: &str) -> (Editor, Compositor, Theme, Config) {
    let mut editor = Editor::new();
    editor.active_buffer_mut().rope = ropey::Rope::from_str(source);
    editor.register_highlights_for_extension(ext);

    let compositor = Compositor::new();
    let theme = Theme::dark();
    let config = Config::default();

    (editor, compositor, theme, config)
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

pub fn stat_avg(v: &[f64]) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    v.iter().sum::<f64>() / v.len() as f64
}

pub fn stat_percentile(v: &mut [f64], pct: f64) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let idx = ((pct / 100.0) * (v.len() - 1) as f64).round() as usize;
    v[idx.min(v.len() - 1)]
}

pub fn format_us(us: f64) -> String {
    if us < 1000.0 {
        format!("{:.0}us", us)
    } else if us < 1_000_000.0 {
        format!("{:.1}ms", us / 1000.0)
    } else {
        format!("{:.2}s", us / 1_000_000.0)
    }
}
