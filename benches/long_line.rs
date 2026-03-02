//! Benchmark: insert_char performance on long lines.
//!
//! Measures dispatch (insert_char + highlight update) and render times
//! as line length grows, to identify scaling bottlenecks.
//!
//! Run: cargo bench --bench bench-long-line

#[path = "common.rs"]
mod common;

use std::time::Instant;

use gargo::core::editor::Editor;
use gargo::input::action::CoreAction;
use gargo::input::chord::KeyState;
use gargo::ui::framework::component::RenderContext;

use common::{NullWriter, format_us, setup_editor, stat_avg, stat_percentile};

// ---------------------------------------------------------------------------
// Source generators
// ---------------------------------------------------------------------------

/// Generate a file with a single very long line (no newlines except at end).
fn generate_single_long_line(char_count: usize) -> String {
    // Realistic Rust-like content: `let x = some_function(arg1, arg2, ...);`
    let pattern = "let result = some_function(alpha, beta, gamma, delta); ";
    let mut out = String::with_capacity(char_count + 1);
    while out.len() < char_count {
        out.push_str(pattern);
    }
    out.truncate(char_count);
    out.push('\n');
    out
}

/// Generate a file where one line (at `long_line_idx`) is very long,
/// surrounded by normal-length lines.
fn generate_mixed_file(
    normal_lines: usize,
    long_line_chars: usize,
    long_line_idx: usize,
) -> String {
    let mut out = String::new();
    for i in 0..normal_lines {
        if i == long_line_idx {
            let pattern = "let result = some_function(alpha, beta, gamma, delta); ";
            let mut line = String::with_capacity(long_line_chars);
            while line.len() < long_line_chars {
                line.push_str(pattern);
            }
            line.truncate(long_line_chars);
            out.push_str(&line);
            out.push('\n');
        } else {
            out.push_str(&format!("    let var_{} = compute_value({});\n", i, i));
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Dispatch helper
// ---------------------------------------------------------------------------

fn dispatch_action(editor: &mut Editor, action: &CoreAction) {
    match action {
        CoreAction::InsertChar(c) => {
            editor.active_buffer_mut().insert_char(*c);
            editor.mark_highlights_dirty();
        }
        CoreAction::DeleteBackward => {
            editor.active_buffer_mut().delete_backward();
            editor.mark_highlights_dirty();
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Benchmark result
// ---------------------------------------------------------------------------

struct BenchResult {
    label: String,
    dispatch_us: Vec<f64>,
    render_us: Vec<f64>,
    total_us: Vec<f64>,
}

fn print_result(r: &mut BenchResult) {
    let d_avg = format_us(stat_avg(&r.dispatch_us));
    let d_p95 = format_us(stat_percentile(&mut r.dispatch_us, 95.0));
    let r_avg = format_us(stat_avg(&r.render_us));
    let r_p95 = format_us(stat_percentile(&mut r.render_us, 95.0));
    let t_avg = format_us(stat_avg(&r.total_us));
    let t_p95 = format_us(stat_percentile(&mut r.total_us, 95.0));

    println!(
        "{:<40} {:>8} {:>8} {:>8} {:>8} {:>8} {:>8}",
        r.label, d_avg, d_p95, r_avg, r_p95, t_avg, t_p95
    );
}

fn print_header() {
    println!(
        "{:<40} {:>8} {:>8} {:>8} {:>8} {:>8} {:>8}",
        "Test", "d.avg", "d.p95", "r.avg", "r.p95", "t.avg", "t.p95"
    );
    println!(
        "{:<40} {:>8} {:>8} {:>8} {:>8} {:>8} {:>8}",
        "", "dispatch", "", "render", "", "total", ""
    );
}

// ---------------------------------------------------------------------------
// Bench 1: InsertChar at different positions in a long line
// ---------------------------------------------------------------------------

fn bench_insert_at_position(
    line_len: usize,
    cursor_frac: f64,
    warmup: usize,
    iterations: usize,
    term_cols: usize,
    term_rows: usize,
) -> BenchResult {
    let source = generate_single_long_line(line_len);
    let (mut editor, mut compositor, theme, config) = setup_editor(&source, "file.rs");

    // Place cursor at given fraction of line
    let cursor_pos = (line_len as f64 * cursor_frac) as usize;
    editor.active_buffer_mut().set_cursor(cursor_pos);
    editor.active_buffer_mut().begin_transaction();

    let key_state = KeyState::Normal;
    let project_root = std::path::Path::new(".");
    let mut null_writer = NullWriter;
    let action = CoreAction::InsertChar('x');

    // Warmup
    for _ in 0..warmup {
        dispatch_action(&mut editor, &action);
        editor.update_highlights_if_dirty();
        let ctx = RenderContext::new(
            term_cols,
            term_rows,
            &editor,
            &theme,
            &key_state,
            &config,
            project_root,
            false,
            false,
        );
        let _ = compositor.render(&ctx, &mut null_writer);
    }

    let mut dispatch_us = Vec::with_capacity(iterations);
    let mut render_us = Vec::with_capacity(iterations);
    let mut total_us = Vec::with_capacity(iterations);

    for _ in 0..iterations {
        let t0 = Instant::now();
        dispatch_action(&mut editor, &action);
        let t1 = Instant::now();

        editor.update_highlights_if_dirty();
        let ctx = RenderContext::new(
            term_cols,
            term_rows,
            &editor,
            &theme,
            &key_state,
            &config,
            project_root,
            false,
            false,
        );
        let _ = compositor.render(&ctx, &mut null_writer);
        let t2 = Instant::now();

        dispatch_us.push(t1.duration_since(t0).as_secs_f64() * 1_000_000.0);
        render_us.push(t2.duration_since(t1).as_secs_f64() * 1_000_000.0);
        total_us.push(t2.duration_since(t0).as_secs_f64() * 1_000_000.0);
    }

    let pos_label = format!("{}%", (cursor_frac * 100.0) as usize);
    BenchResult {
        label: format!("insert@{} (line={}ch)", pos_label, line_len),
        dispatch_us,
        render_us,
        total_us,
    }
}

// ---------------------------------------------------------------------------
// Bench 2: Sustained typing on a growing line
// ---------------------------------------------------------------------------

fn bench_sustained_typing(
    initial_line_len: usize,
    type_count: usize,
    term_cols: usize,
    term_rows: usize,
) -> BenchResult {
    let source = generate_single_long_line(initial_line_len);
    let (mut editor, mut compositor, theme, config) = setup_editor(&source, "file.rs");

    // Place cursor at end of line
    editor.active_buffer_mut().set_cursor(initial_line_len);
    editor.active_buffer_mut().begin_transaction();

    let key_state = KeyState::Normal;
    let project_root = std::path::Path::new(".");
    let mut null_writer = NullWriter;
    let action = CoreAction::InsertChar('a');

    let mut total_us = Vec::with_capacity(type_count);
    let mut dispatch_us = Vec::with_capacity(type_count);
    let mut render_us = Vec::with_capacity(type_count);

    for _ in 0..type_count {
        let t0 = Instant::now();
        dispatch_action(&mut editor, &action);
        let t1 = Instant::now();

        editor.update_highlights_if_dirty();
        let ctx = RenderContext::new(
            term_cols,
            term_rows,
            &editor,
            &theme,
            &key_state,
            &config,
            project_root,
            false,
            false,
        );
        let _ = compositor.render(&ctx, &mut null_writer);
        let t2 = Instant::now();

        dispatch_us.push(t1.duration_since(t0).as_secs_f64() * 1_000_000.0);
        render_us.push(t2.duration_since(t1).as_secs_f64() * 1_000_000.0);
        total_us.push(t2.duration_since(t0).as_secs_f64() * 1_000_000.0);
    }

    BenchResult {
        label: format!(
            "sustained typing (start={}ch, +{})",
            initial_line_len, type_count
        ),
        dispatch_us,
        render_us,
        total_us,
    }
}

// ---------------------------------------------------------------------------
// Bench 3: Insert in a mixed file (long line among normal lines)
// ---------------------------------------------------------------------------

fn bench_mixed_file_insert(
    long_line_chars: usize,
    warmup: usize,
    iterations: usize,
    term_cols: usize,
    term_rows: usize,
) -> BenchResult {
    let total_lines = 500;
    let long_line_idx = 10; // Long line near top so it's visible
    let source = generate_mixed_file(total_lines, long_line_chars, long_line_idx);
    let (mut editor, mut compositor, theme, config) = setup_editor(&source, "file.rs");

    // Place cursor in the middle of the long line
    let long_line_start = editor.active_buffer().rope.line_to_char(long_line_idx);
    editor
        .active_buffer_mut()
        .set_cursor(long_line_start + long_line_chars / 2);
    editor.active_buffer_mut().begin_transaction();

    let key_state = KeyState::Normal;
    let project_root = std::path::Path::new(".");
    let mut null_writer = NullWriter;
    let action = CoreAction::InsertChar('z');

    // Warmup
    for _ in 0..warmup {
        dispatch_action(&mut editor, &action);
        editor.update_highlights_if_dirty();
        let ctx = RenderContext::new(
            term_cols,
            term_rows,
            &editor,
            &theme,
            &key_state,
            &config,
            project_root,
            false,
            false,
        );
        let _ = compositor.render(&ctx, &mut null_writer);
    }

    let mut dispatch_us = Vec::with_capacity(iterations);
    let mut render_us = Vec::with_capacity(iterations);
    let mut total_us = Vec::with_capacity(iterations);

    for _ in 0..iterations {
        let t0 = Instant::now();
        dispatch_action(&mut editor, &action);
        let t1 = Instant::now();

        editor.update_highlights_if_dirty();
        let ctx = RenderContext::new(
            term_cols,
            term_rows,
            &editor,
            &theme,
            &key_state,
            &config,
            project_root,
            false,
            false,
        );
        let _ = compositor.render(&ctx, &mut null_writer);
        let t2 = Instant::now();

        dispatch_us.push(t1.duration_since(t0).as_secs_f64() * 1_000_000.0);
        render_us.push(t2.duration_since(t1).as_secs_f64() * 1_000_000.0);
        total_us.push(t2.duration_since(t0).as_secs_f64() * 1_000_000.0);
    }

    BenchResult {
        label: format!("mixed file insert (longline={}ch)", long_line_chars),
        dispatch_us,
        render_us,
        total_us,
    }
}

// ---------------------------------------------------------------------------
// Bench 4: Dispatch-only (no render) to isolate insert_char cost
// ---------------------------------------------------------------------------

fn bench_insert_char_only(line_len: usize, cursor_frac: f64, iterations: usize) -> BenchResult {
    let source = generate_single_long_line(line_len);
    let (mut editor, _, _, _) = setup_editor(&source, "file.rs");

    let cursor_pos = (line_len as f64 * cursor_frac) as usize;
    editor.active_buffer_mut().set_cursor(cursor_pos);
    editor.active_buffer_mut().begin_transaction();

    let mut dispatch_us = Vec::with_capacity(iterations);

    for _ in 0..iterations {
        let t0 = Instant::now();
        editor.active_buffer_mut().insert_char('x');
        let t1 = Instant::now();
        dispatch_us.push(t1.duration_since(t0).as_secs_f64() * 1_000_000.0);
    }

    BenchResult {
        label: format!("insert_char only (line={}ch, mid)", line_len),
        dispatch_us: dispatch_us.clone(),
        render_us: vec![0.0; iterations],
        total_us: dispatch_us,
    }
}

// ---------------------------------------------------------------------------
// Bench 5: Render-only to isolate rendering cost for long lines
// ---------------------------------------------------------------------------

fn bench_render_only(
    line_len: usize,
    iterations: usize,
    term_cols: usize,
    term_rows: usize,
) -> BenchResult {
    let source = generate_single_long_line(line_len);
    let (mut editor, mut compositor, theme, config) = setup_editor(&source, "file.rs");

    editor.active_buffer_mut().set_cursor(line_len / 2);
    // Ensure highlights are fresh before render-only loop
    editor.mark_highlights_dirty();
    editor.update_highlights_if_dirty();

    let key_state = KeyState::Normal;
    let project_root = std::path::Path::new(".");
    let mut null_writer = NullWriter;

    let mut render_us = Vec::with_capacity(iterations);

    for _ in 0..iterations {
        let t0 = Instant::now();
        let ctx = RenderContext::new(
            term_cols,
            term_rows,
            &editor,
            &theme,
            &key_state,
            &config,
            project_root,
            false,
            false,
        );
        let _ = compositor.render(&ctx, &mut null_writer);
        let t1 = Instant::now();
        render_us.push(t1.duration_since(t0).as_secs_f64() * 1_000_000.0);
    }

    BenchResult {
        label: format!("render only (line={}ch)", line_len),
        dispatch_us: vec![0.0; iterations],
        render_us: render_us.clone(),
        total_us: render_us,
    }
}

// ---------------------------------------------------------------------------
// Bench 6: Highlight update only (tree-sitter reparse after edit)
// ---------------------------------------------------------------------------

fn bench_highlight_update_only(line_len: usize, iterations: usize) -> BenchResult {
    let source = generate_single_long_line(line_len);
    let (mut editor, _, _, _) = setup_editor(&source, "file.rs");

    editor.active_buffer_mut().set_cursor(line_len / 2);
    editor.active_buffer_mut().begin_transaction();

    let mut dispatch_us = Vec::with_capacity(iterations);

    for _ in 0..iterations {
        editor.active_buffer_mut().insert_char('x');
        editor.mark_highlights_dirty();

        let t0 = Instant::now();
        editor.update_highlights_if_dirty();
        let t1 = Instant::now();

        dispatch_us.push(t1.duration_since(t0).as_secs_f64() * 1_000_000.0);
    }

    BenchResult {
        label: format!("highlight update (line={}ch)", line_len),
        dispatch_us: dispatch_us.clone(),
        render_us: vec![0.0; iterations],
        total_us: dispatch_us,
    }
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

fn main() {
    let warmup = 10;
    let iterations = 200;
    let term_cols: usize = 120;
    let term_rows: usize = 40;

    println!("Gargo Benchmark: Long Line Insert Performance (release build)");
    println!(
        "Terminal: {}x{}, Iterations: {}, Warmup: {}",
        term_cols, term_rows, iterations, warmup
    );

    // === Section 1: Insert at different positions, varying line length ===
    println!();
    println!("=== Insert at cursor position (full round-trip: dispatch + highlight + render) ===");
    print_header();

    let line_lengths = [100, 500, 1_000, 5_000, 10_000, 50_000];
    let cursor_positions = [0.0, 0.5, 1.0]; // start, middle, end

    for &len in &line_lengths {
        for &frac in &cursor_positions {
            let mut r =
                bench_insert_at_position(len, frac, warmup, iterations, term_cols, term_rows);
            print_result(&mut r);
        }
    }

    // === Section 2: Dispatch-only (isolate insert_char) ===
    println!();
    println!("=== insert_char() only (no highlight, no render) ===");
    print_header();

    for &len in &line_lengths {
        let mut r = bench_insert_char_only(len, 0.5, iterations);
        print_result(&mut r);
    }

    // === Section 3: Render-only (isolate rendering) ===
    println!();
    println!("=== Render only (no edits, just rendering a long line) ===");
    print_header();

    for &len in &line_lengths {
        let mut r = bench_render_only(len, iterations, term_cols, term_rows);
        print_result(&mut r);
    }

    // === Section 4: Highlight update only ===
    println!();
    println!("=== Highlight update only (tree-sitter reparse after insert) ===");
    print_header();

    for &len in &[100, 1_000, 10_000, 50_000] {
        let mut r = bench_highlight_update_only(len, iterations);
        print_result(&mut r);
    }

    // === Section 5: Sustained typing simulation ===
    println!();
    println!("=== Sustained typing (append chars, line grows each iteration) ===");
    print_header();

    for &start_len in &[100, 1_000, 10_000] {
        let mut r = bench_sustained_typing(start_len, 200, term_cols, term_rows);
        print_result(&mut r);
    }

    // === Section 6: Mixed file (long line among normal lines) ===
    println!();
    println!("=== Mixed file (one long line among 500 normal lines, insert mid-line) ===");
    print_header();

    for &long_len in &[500, 5_000, 50_000] {
        let mut r = bench_mixed_file_insert(long_len, warmup, iterations, term_cols, term_rows);
        print_result(&mut r);
    }
}
