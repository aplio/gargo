//! Scroll benchmark: measures frame-by-frame MoveDown from top to bottom
//! of large markdown documents, simulating holding `j`.
//!
//! Run: cargo run --example bench-scroll --release

#[path = "common.rs"]
mod common;

use std::time::Instant;

use gargo::input::chord::KeyState;
use gargo::ui::framework::component::RenderContext;

use common::{NullWriter, format_us, setup_editor, stat_avg, stat_percentile};

// ---------------------------------------------------------------------------
// Deterministic markdown generator
// ---------------------------------------------------------------------------

fn generate_markdown(lines: usize) -> String {
    let mut out = String::new();
    let mut line_count = 0;
    let mut section = 0;

    while line_count < lines {
        // Heading
        let depth = (section % 3) + 1;
        let hashes = "#".repeat(depth);
        out.push_str(&format!("{hashes} Section {section}\n\n"));
        line_count += 2;
        if line_count >= lines {
            break;
        }

        // Paragraph
        for s in 0..3 {
            out.push_str(&format!(
                "This is paragraph text for section {section}, sentence {s}. \
                 Lorem ipsum dolor sit amet, consectetur adipiscing elit. \
                 Sed do eiusmod tempor incididunt ut labore et dolore magna aliqua.\n"
            ));
            line_count += 1;
            if line_count >= lines {
                break;
            }
        }
        if line_count >= lines {
            break;
        }
        out.push('\n');
        line_count += 1;
        if line_count >= lines {
            break;
        }

        // Bullet list
        for i in 0..4 {
            out.push_str(&format!(
                "- Item {i} in section {section}: some detail here\n"
            ));
            line_count += 1;
            if line_count >= lines {
                break;
            }
        }
        if line_count >= lines {
            break;
        }
        out.push('\n');
        line_count += 1;
        if line_count >= lines {
            break;
        }

        // Code block
        out.push_str("```rust\n");
        line_count += 1;
        if line_count >= lines {
            break;
        }
        out.push_str(&format!("fn example_{section}() -> i32 {{\n"));
        line_count += 1;
        if line_count >= lines {
            break;
        }
        out.push_str("    let x = 42;\n");
        line_count += 1;
        if line_count >= lines {
            break;
        }
        out.push_str("    x * 2\n");
        line_count += 1;
        if line_count >= lines {
            break;
        }
        out.push_str("}\n");
        line_count += 1;
        if line_count >= lines {
            break;
        }
        out.push_str("```\n\n");
        line_count += 2;
        if line_count >= lines {
            break;
        }

        // Blockquote
        out.push_str(&format!("> Notable quote from section {section}.\n"));
        line_count += 1;
        if line_count >= lines {
            break;
        }
        out.push_str("> With a second line for emphasis.\n\n");
        line_count += 2;

        section += 1;
    }

    out
}

// ---------------------------------------------------------------------------
// Scroll benchmark: MoveDown from line 0 to EOF, one render per step
// ---------------------------------------------------------------------------

fn bench_scroll(source: &str, ext: &str, term_cols: usize, term_rows: usize) -> (usize, Vec<f64>) {
    let (mut editor, mut compositor, theme, config) = setup_editor(source, ext);
    let key_state = KeyState::Normal;
    let project_root = std::path::Path::new(".");
    let mut null_writer = NullWriter;
    let view_height = if term_rows > 1 { term_rows - 1 } else { 1 };

    let total_lines = editor.active_buffer().rope.len_lines();

    // Warmup: render initial frame
    editor
        .active_buffer_mut()
        .ensure_cursor_visible(view_height);
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

    let mut frame_us: Vec<f64> = Vec::with_capacity(total_lines);

    // Scroll down one line at a time, rendering each frame
    for _ in 0..total_lines.saturating_sub(1) {
        let t0 = Instant::now();

        editor.active_buffer_mut().move_down();
        editor
            .active_buffer_mut()
            .ensure_cursor_visible(view_height);
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

        let elapsed = t0.elapsed();
        frame_us.push(elapsed.as_secs_f64() * 1_000_000.0);
    }

    (total_lines, frame_us)
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

fn main() {
    let file_sizes = [1000, 5000, 10_000];
    let term_cols: usize = 120;
    let term_rows: usize = 40;

    println!("Gargo Benchmark: Markdown Scroll (release build)");
    println!("Terminal: {}x{}", term_cols, term_rows);
    println!("Simulates holding `j` from top to bottom of file.");
    println!();
    println!(
        "{:>8} {:>8} {:>8} {:>8} {:>8} {:>8} {:>8}",
        "lines", "frames", "total", "f.avg", "f.p50", "f.p95", "f.p99"
    );

    for &size in &file_sizes {
        let source = generate_markdown(size);

        let (total_lines, mut frame_us) = bench_scroll(&source, "file.md", term_cols, term_rows);
        let frames = frame_us.len();

        let total: f64 = frame_us.iter().sum();
        let avg = stat_avg(&frame_us);
        let p50 = stat_percentile(&mut frame_us, 50.0);
        let p95 = stat_percentile(&mut frame_us, 95.0);
        let p99 = stat_percentile(&mut frame_us, 99.0);

        println!(
            "{:>8} {:>8} {:>8} {:>8} {:>8} {:>8} {:>8}",
            total_lines,
            frames,
            format_us(total),
            format_us(avg),
            format_us(p50),
            format_us(p95),
            format_us(p99),
        );
    }
}
