//! Buffer picker benchmark: measures fuzzy matching, filtering, preview updates,
//! and rendering for the buffer picker palette across varying buffer counts.
//!
//! Run: cargo run --example bench-buffer-picker --release

#[path = "common.rs"]
mod common;

use std::time::Instant;

use gargo::syntax::theme::Theme;
use gargo::ui::framework::surface::Surface;
use gargo::ui::overlays::palette::Palette;
use gargo::ui::shared::filtering::fuzzy_match;

use common::{format_us, stat_avg, stat_percentile};

// ---------------------------------------------------------------------------
// Deterministic buffer entry generator
// ---------------------------------------------------------------------------

/// Generate realistic buffer entries: (id, name, preview_lines).
/// Names look like "src/services/auth/handler.rs", "[scratch]", etc.
fn generate_buffer_entries(count: usize) -> Vec<(usize, String, Vec<String>)> {
    let dirs = [
        "src",
        "src/core",
        "src/ui",
        "src/input",
        "src/syntax",
        "src/config",
        "src/services/auth",
        "src/services/api",
        "src/utils",
        "tests",
        "tests/integration",
        "benches",
        "examples",
        "docs",
        "src/ui/components",
        "src/core/buffer",
    ];
    let files = [
        "mod.rs",
        "lib.rs",
        "main.rs",
        "handler.rs",
        "config.rs",
        "editor.rs",
        "palette.rs",
        "keymap.rs",
        "theme.rs",
        "surface.rs",
        "document.rs",
        "highlight.rs",
        "compositor.rs",
        "action.rs",
        "registry.rs",
        "util.rs",
        "render.rs",
        "state.rs",
        "app.rs",
        "buffer.rs",
    ];

    let preview_line = |i: usize| -> Vec<String> {
        (0..20)
            .map(|l| format!("    // line {} of buffer {}", l, i))
            .collect()
    };

    let mut entries = Vec::with_capacity(count);

    // First entry is always [scratch]
    if count > 0 {
        entries.push((0, "[scratch]".to_string(), preview_line(0)));
    }

    for i in 1..count {
        let dir = dirs[i % dirs.len()];
        let file = files[i % files.len()];
        let name = if i % 7 == 0 {
            // Some buffers have deeper paths
            format!("{}/sub_{}/{}", dir, i, file)
        } else {
            format!("{}/{}", dir, file)
        };
        entries.push((i, name, preview_line(i)));
    }

    entries
}

// ---------------------------------------------------------------------------
// Benchmark: fuzzy_match standalone
// ---------------------------------------------------------------------------

fn bench_fuzzy_match(
    haystacks: &[String],
    needle: &str,
    warmup: usize,
    iterations: usize,
) -> Vec<f64> {
    // Warmup
    for _ in 0..warmup {
        for h in haystacks {
            let _ = fuzzy_match(h, needle);
        }
    }

    let mut times = Vec::with_capacity(iterations);
    for _ in 0..iterations {
        let t0 = Instant::now();
        for h in haystacks {
            let _ = fuzzy_match(h, needle);
        }
        times.push(t0.elapsed().as_secs_f64() * 1_000_000.0);
    }
    times
}

// ---------------------------------------------------------------------------
// Benchmark: full keystroke cycle (filter + preview)
// ---------------------------------------------------------------------------

fn bench_keystroke_cycle(
    entries: &[(usize, String, Vec<String>)],
    query: &str,
    warmup: usize,
    iterations: usize,
) -> (Vec<f64>, Vec<f64>) {
    let mut filter_us = Vec::with_capacity(iterations);
    let mut total_us = Vec::with_capacity(iterations);

    // Warmup
    for _ in 0..warmup {
        let mut palette = Palette::new_buffer_picker(entries.to_vec());
        for c in query.chars() {
            palette.on_char_buffer(c);
        }
    }

    for _ in 0..iterations {
        let mut palette = Palette::new_buffer_picker(entries.to_vec());

        // Measure typing each character (cumulative filter + preview)
        let t0 = Instant::now();
        for c in query.chars() {
            palette.on_char_buffer(c);
        }
        let t_filter = t0.elapsed();
        filter_us.push(t_filter.as_secs_f64() * 1_000_000.0);

        // Total includes the initial construction too
        total_us.push(t_filter.as_secs_f64() * 1_000_000.0);
    }

    (filter_us, total_us)
}

// ---------------------------------------------------------------------------
// Benchmark: render overlay
// ---------------------------------------------------------------------------

fn bench_render(
    entries: &[(usize, String, Vec<String>)],
    query: &str,
    warmup: usize,
    iterations: usize,
    term_cols: usize,
    term_rows: usize,
) -> Vec<f64> {
    let theme = Theme::dark();

    // Setup: create palette in steady state
    let mut palette = Palette::new_buffer_picker(entries.to_vec());
    for c in query.chars() {
        palette.on_char_buffer(c);
    }

    let mut surface = Surface::new(term_cols, term_rows);

    // Warmup
    for _ in 0..warmup {
        palette.render_overlay(&mut surface, &theme);
    }

    let mut times = Vec::with_capacity(iterations);
    for _ in 0..iterations {
        surface = Surface::new(term_cols, term_rows);
        let t0 = Instant::now();
        palette.render_overlay(&mut surface, &theme);
        times.push(t0.elapsed().as_secs_f64() * 1_000_000.0);
    }
    times
}

// ---------------------------------------------------------------------------
// Benchmark: incremental typing (per-character cost)
// ---------------------------------------------------------------------------

fn bench_incremental_typing(
    entries: &[(usize, String, Vec<String>)],
    query: &str,
    warmup: usize,
    iterations: usize,
) -> Vec<(char, Vec<f64>)> {
    let chars: Vec<char> = query.chars().collect();
    let mut results: Vec<(char, Vec<f64>)> = chars
        .iter()
        .map(|&c| (c, Vec::with_capacity(iterations)))
        .collect();

    // Warmup
    for _ in 0..warmup {
        let mut palette = Palette::new_buffer_picker(entries.to_vec());
        for &c in &chars {
            palette.on_char_buffer(c);
        }
    }

    for _ in 0..iterations {
        let mut palette = Palette::new_buffer_picker(entries.to_vec());
        for (i, &c) in chars.iter().enumerate() {
            let t0 = Instant::now();
            palette.on_char_buffer(c);
            results[i].1.push(t0.elapsed().as_secs_f64() * 1_000_000.0);
        }
    }

    results
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

fn main() {
    let buffer_counts = [10, 50, 200, 1000];
    let warmup = 10;
    let iterations = 100;
    let term_cols: usize = 120;
    let term_rows: usize = 40;

    println!("Gargo Benchmark: Buffer Picker (release build)");
    println!(
        "Terminal: {}x{}, Iterations: {}, Warmup: {}",
        term_cols, term_rows, iterations, warmup
    );

    // -----------------------------------------------------------------------
    // 1. fuzzy_match micro-benchmark
    // -----------------------------------------------------------------------
    println!();
    println!("=== fuzzy_match: full scan over N buffer names ===");
    println!(
        "{:>8} {:>12} {:>8} {:>8} {:>8}",
        "buffers", "needle", "avg", "p95", "p99"
    );

    let queries = [
        ("", "empty"),
        ("m", "1-char"),
        ("main", "4-char"),
        ("src/pal", "path"),
    ];

    for &count in &buffer_counts {
        let entries = generate_buffer_entries(count);
        let haystacks: Vec<String> = entries.iter().map(|(_, name, _)| name.clone()).collect();

        for &(needle, label) in &queries {
            let mut times = bench_fuzzy_match(&haystacks, needle, warmup, iterations);
            let avg = format_us(stat_avg(&times));
            let p95 = format_us(stat_percentile(&mut times, 95.0));
            let p99 = format_us(stat_percentile(&mut times, 99.0));
            println!(
                "{:>8} {:>12} {:>8} {:>8} {:>8}",
                count, label, avg, p95, p99
            );
        }
    }

    // -----------------------------------------------------------------------
    // 2. Full keystroke cycle (filter + preview)
    // -----------------------------------------------------------------------
    println!();
    println!("=== keystroke cycle: type query into buffer picker (filter + preview) ===");
    println!(
        "{:>8} {:>12} {:>8} {:>8} {:>8}",
        "buffers", "query", "avg", "p95", "p99"
    );

    let typed_queries = [("m", "\"m\""), ("main", "\"main\""), ("src/p", "\"src/p\"")];

    for &count in &buffer_counts {
        let entries = generate_buffer_entries(count);

        for &(query, label) in &typed_queries {
            let (mut filter_times, _) = bench_keystroke_cycle(&entries, query, warmup, iterations);
            let avg = format_us(stat_avg(&filter_times));
            let p95 = format_us(stat_percentile(&mut filter_times, 95.0));
            let p99 = format_us(stat_percentile(&mut filter_times, 99.0));
            println!(
                "{:>8} {:>12} {:>8} {:>8} {:>8}",
                count, label, avg, p95, p99
            );
        }
    }

    // -----------------------------------------------------------------------
    // 3. Render overlay
    // -----------------------------------------------------------------------
    println!();
    println!("=== render: buffer picker overlay ===");
    println!(
        "{:>8} {:>12} {:>8} {:>8} {:>8}",
        "buffers", "query", "avg", "p95", "p99"
    );

    let render_queries = [("", "empty"), ("main", "\"main\"")];

    for &count in &buffer_counts {
        let entries = generate_buffer_entries(count);

        for &(query, label) in &render_queries {
            let mut times = bench_render(&entries, query, warmup, iterations, term_cols, term_rows);
            let avg = format_us(stat_avg(&times));
            let p95 = format_us(stat_percentile(&mut times, 95.0));
            let p99 = format_us(stat_percentile(&mut times, 99.0));
            println!(
                "{:>8} {:>12} {:>8} {:>8} {:>8}",
                count, label, avg, p95, p99
            );
        }
    }

    // -----------------------------------------------------------------------
    // 4. Incremental typing breakdown (per-character cost)
    // -----------------------------------------------------------------------
    println!();
    println!("=== incremental: per-character cost for \"main.rs\" with 200 buffers ===");
    println!("{:>6} {:>8} {:>8} {:>8}", "char", "avg", "p95", "p99");

    let entries = generate_buffer_entries(200);
    let mut char_results = bench_incremental_typing(&entries, "main.rs", warmup, iterations);
    for (c, times) in &mut char_results {
        let avg = format_us(stat_avg(times));
        let p95 = format_us(stat_percentile(times, 95.0));
        let p99 = format_us(stat_percentile(times, 99.0));
        println!("{:>6} {:>8} {:>8} {:>8}", format!("'{}'", c), avg, p95, p99);
    }
}
