//! Per-action benchmark: measures dispatch + render for individual editor actions
//! across varying file sizes.
//!
//! Run: cargo run --example bench-actions --release

#[path = "common.rs"]
mod common;

use std::time::Instant;

use gargo::config::Config;
use gargo::core::editor::Editor;
use gargo::input::action::CoreAction;
use gargo::input::chord::KeyState;
use gargo::syntax::theme::Theme;
use gargo::ui::framework::component::RenderContext;
use gargo::ui::framework::compositor::Compositor;

use common::{NullWriter, format_us, setup_editor, stat_avg, stat_percentile};

// ---------------------------------------------------------------------------
// Deterministic Rust source generator
// ---------------------------------------------------------------------------

fn generate_rust_source(lines: usize) -> String {
    let mut out = String::new();
    let mut line_count = 0;

    let mut struct_id = 0;
    let mut fn_id = 0;

    while line_count < lines {
        out.push_str(&format!("/// Documentation for Struct{struct_id}.\n"));
        line_count += 1;
        if line_count >= lines {
            break;
        }

        out.push_str(&format!("pub struct Struct{struct_id} {{\n"));
        line_count += 1;
        if line_count >= lines {
            break;
        }

        for field in 0..3 {
            out.push_str(&format!("    pub field_{field}: i64,\n"));
            line_count += 1;
            if line_count >= lines {
                break;
            }
        }
        if line_count >= lines {
            break;
        }

        out.push_str("}\n\n");
        line_count += 2;
        if line_count >= lines {
            break;
        }

        out.push_str(&format!("impl Struct{struct_id} {{\n"));
        line_count += 1;
        if line_count >= lines {
            break;
        }

        out.push_str("    pub fn new() -> Self {\n");
        line_count += 1;
        if line_count >= lines {
            break;
        }

        out.push_str("        Self { field_0: 0, field_1: 1, field_2: 2 }\n");
        line_count += 1;
        if line_count >= lines {
            break;
        }

        out.push_str("    }\n\n");
        line_count += 2;
        if line_count >= lines {
            break;
        }

        out.push_str("    pub fn compute(&self) -> i64 {\n");
        line_count += 1;
        if line_count >= lines {
            break;
        }

        out.push_str("        self.field_0 + self.field_1 * self.field_2\n");
        line_count += 1;
        if line_count >= lines {
            break;
        }

        out.push_str("    }\n}\n\n");
        line_count += 3;
        struct_id += 1;

        if line_count >= lines {
            break;
        }

        out.push_str(&format!("/// Standalone function number {fn_id}.\n"));
        line_count += 1;
        if line_count >= lines {
            break;
        }

        out.push_str(&format!(
            "pub fn compute_{fn_id}(x: i64, y: i64) -> i64 {{\n"
        ));
        line_count += 1;
        if line_count >= lines {
            break;
        }

        out.push_str("    let mut result = 0;\n");
        line_count += 1;
        if line_count >= lines {
            break;
        }

        out.push_str("    for i in 0..x {\n");
        line_count += 1;
        if line_count >= lines {
            break;
        }

        out.push_str("        result += i * y;\n");
        line_count += 1;
        if line_count >= lines {
            break;
        }

        out.push_str("    }\n");
        line_count += 1;
        if line_count >= lines {
            break;
        }

        out.push_str("    result\n");
        line_count += 1;
        if line_count >= lines {
            break;
        }

        out.push_str("}\n\n");
        line_count += 2;
        fn_id += 1;
    }

    out
}

// ---------------------------------------------------------------------------
// Dispatch – mirrors App::dispatch for the subset of benchmarked actions
// ---------------------------------------------------------------------------

fn dispatch_action(editor: &mut Editor, action: &CoreAction) {
    match action {
        CoreAction::MoveDown => editor.active_buffer_mut().move_down(),
        CoreAction::MoveUp => editor.active_buffer_mut().move_up(),
        CoreAction::MoveToFileEnd => editor.active_buffer_mut().move_to_file_end(),
        CoreAction::MoveToFileStart => editor.active_buffer_mut().move_to_file_start(),
        CoreAction::InsertChar(c) => {
            editor.active_buffer_mut().insert_char(*c);
            editor.mark_highlights_dirty();
        }
        CoreAction::DeleteBackward => {
            editor.active_buffer_mut().delete_backward();
            editor.mark_highlights_dirty();
        }
        CoreAction::Undo => {
            if editor.active_buffer_mut().undo() {
                editor.mark_highlights_dirty();
            }
        }
        CoreAction::SearchUpdate(pattern) => {
            editor.search_update(pattern);
            editor.search_next();
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Benchmark runner
// ---------------------------------------------------------------------------

struct BenchResult {
    action_name: String,
    dispatch_us: Vec<f64>,
    render_us: Vec<f64>,
    total_us: Vec<f64>,
}

#[allow(clippy::too_many_arguments)]
fn bench_action(
    editor: &mut Editor,
    compositor: &mut Compositor,
    theme: &Theme,
    config: &Config,
    action: &CoreAction,
    action_name: &str,
    warmup: usize,
    iterations: usize,
    batch: usize,
    term_cols: usize,
    term_rows: usize,
) -> BenchResult {
    let key_state = KeyState::Normal;
    let project_root = std::path::Path::new(".");
    let mut null_writer = NullWriter;

    // Warmup
    for _ in 0..warmup {
        for _ in 0..batch {
            dispatch_action(editor, action);
        }
        editor.update_highlights_if_dirty();
        let ctx = RenderContext::new(
            term_cols,
            term_rows,
            editor,
            theme,
            &key_state,
            config,
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
        for _ in 0..batch {
            dispatch_action(editor, action);
        }
        let t1 = Instant::now();
        editor.update_highlights_if_dirty();
        let ctx = RenderContext::new(
            term_cols,
            term_rows,
            editor,
            theme,
            &key_state,
            config,
            project_root,
            false,
            false,
        );
        let _ = compositor.render(&ctx, &mut null_writer);
        let t2 = Instant::now();

        let d = t1.duration_since(t0);
        let r = t2.duration_since(t1);
        dispatch_us.push(d.as_secs_f64() * 1_000_000.0);
        render_us.push(r.as_secs_f64() * 1_000_000.0);
        total_us.push((d + r).as_secs_f64() * 1_000_000.0);
    }

    BenchResult {
        action_name: action_name.to_string(),
        dispatch_us,
        render_us,
        total_us,
    }
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

fn main() {
    let file_sizes = [100, 1000, 5000, 10_000];
    let warmup = 10;
    let iterations = 100;
    let term_cols: usize = 120;
    let term_rows: usize = 40;

    // (name, action, batch_count)
    let actions: Vec<(&str, CoreAction, usize)> = vec![
        ("MoveDown", CoreAction::MoveDown, 1),
        ("MoveUp", CoreAction::MoveUp, 1),
        ("MoveToFileEnd", CoreAction::MoveToFileEnd, 1),
        ("MoveToFileStart", CoreAction::MoveToFileStart, 1),
        ("InsertChar", CoreAction::InsertChar('x'), 1),
        ("InsertChar(x5)", CoreAction::InsertChar('x'), 5),
        ("DeleteBackward", CoreAction::DeleteBackward, 1),
        ("Undo", CoreAction::Undo, 1),
        (
            "SearchUpdate",
            CoreAction::SearchUpdate("fn".to_string()),
            1,
        ),
    ];

    println!("Gargo Benchmark: Per-Action (release build)");
    println!(
        "Terminal: {}x{}, Iterations: {}, Warmup: {}",
        term_cols, term_rows, iterations, warmup
    );

    for &size in &file_sizes {
        println!();
        println!("--- {} lines ---", size);
        println!(
            "{:<20} {:>8} {:>8} {:>8} {:>8} {:>8} {:>8}",
            "Action", "d.avg", "d.p95", "r.avg", "r.p95", "t.avg", "t.p95"
        );
        println!(
            "{:<20} {:>8} {:>8} {:>8} {:>8} {:>8} {:>8}",
            "", "dispatch", "", "render", "", "total", ""
        );

        let source = generate_rust_source(size);

        for (name, action, batch) in &actions {
            let (mut editor, mut compositor, theme, config) = setup_editor(&source, "file.rs");

            if matches!(action, CoreAction::InsertChar(_)) {
                editor.active_buffer_mut().begin_transaction();
            }

            let mut result = bench_action(
                &mut editor,
                &mut compositor,
                &theme,
                &config,
                action,
                name,
                warmup,
                iterations,
                *batch,
                term_cols,
                term_rows,
            );

            let d_avg = format_us(stat_avg(&result.dispatch_us));
            let d_p95 = format_us(stat_percentile(&mut result.dispatch_us, 95.0));
            let r_avg = format_us(stat_avg(&result.render_us));
            let r_p95 = format_us(stat_percentile(&mut result.render_us, 95.0));
            let t_avg = format_us(stat_avg(&result.total_us));
            let t_p95 = format_us(stat_percentile(&mut result.total_us, 95.0));

            println!(
                "{:<20} {:>8} {:>8} {:>8} {:>8} {:>8} {:>8}",
                result.action_name, d_avg, d_p95, r_avg, r_p95, t_avg, t_p95
            );
        }
    }
}
