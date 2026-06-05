//! Server hot-path benchmark: measures the backend work behind the slowest
//! `gargo --server` endpoints, isolated from HTTP/transport so we can optimize
//! the actual cost centers.
//!
//! A HAR captured from a real browsing session showed nearly all request time
//! is server-side `wait` (not transfer), dominated by:
//! - `/api/files` ~120ms each, called repeatedly, never cached
//!   -> `project::collect_files`
//! - `/blob/...` ~217ms, `/api/highlight` ~55ms
//!   -> `syntax::highlight::highlight_text` (tree-sitter)
//!
//! This bench exercises those two functions against THIS repo so numbers are
//! reproducible and reflect a realistic working tree.
//!
//! A later HAR showed the `/status`, `/branches`, and commit pages were
//! dominated by read-only git work. The final section measures the public
//! gix-backed helpers now used by those paths.
//!
//! Run: cargo run --bench bench-server --release
//!  (or: cargo bench --bench bench-server)

#[path = "common.rs"]
mod common;

use std::path::{Path, PathBuf};
use std::time::Instant;

use gargo::command::git;
use gargo::project::collect_files;
use gargo::syntax::highlight::highlight_text;
use gargo::syntax::language::LanguageRegistry;

use common::{format_us, stat_avg, stat_percentile};

/// The gargo repo root (where this bench is compiled from), used as a real
/// git working tree for the `collect_files` benchmark.
fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

// ---------------------------------------------------------------------------
// Benchmark: collect_files  (/api/files hot path)
// ---------------------------------------------------------------------------

/// Each call spawns `git ls-files --cached --others --exclude-standard` AND
/// `git ls-files --deleted`, then filters. No caching today: the editor calls
/// `/api/files` on every Cmd+P open, so this runs in full each time.
fn bench_collect_files(root: &Path, warmup: usize, iterations: usize) -> (Vec<f64>, usize) {
    let mut count = 0;
    for _ in 0..warmup {
        count = collect_files(root).len();
    }

    let mut times = Vec::with_capacity(iterations);
    for _ in 0..iterations {
        let t0 = Instant::now();
        let files = collect_files(root);
        times.push(t0.elapsed().as_secs_f64() * 1_000_000.0);
        count = files.len();
    }
    (times, count)
}

// ---------------------------------------------------------------------------
// Benchmark: highlight_text  (/api/highlight, /blob render)
// ---------------------------------------------------------------------------

/// `highlight_text` memoizes by (text, language), so a fixed input hits cache
/// after the first call. The editor highlights *different* content on every
/// request, so the realistic cost is the cache-MISS path: build a tree-sitter
/// `Parser`, compile the `Query`, parse, and walk captures. We force a miss
/// each iteration by prepending a unique comment line.
fn bench_highlight_miss(
    source: &str,
    lang: &gargo::syntax::language::LanguageDef,
    warmup: usize,
    iterations: usize,
    mut salt: usize,
) -> Vec<f64> {
    for _ in 0..warmup {
        let text = format!("// warm {salt}\n{source}");
        let _ = highlight_text(&text, lang);
        salt += 1;
    }

    let mut times = Vec::with_capacity(iterations);
    for _ in 0..iterations {
        let text = format!("// iter {salt}\n{source}");
        salt += 1;
        let t0 = Instant::now();
        let spans = highlight_text(&text, lang);
        times.push(t0.elapsed().as_secs_f64() * 1_000_000.0);
        std::hint::black_box(&spans);
    }
    times
}

/// Steady-state (cache-HIT) cost for the same input: what a repeat view of an
/// unchanged file costs. Shows the headroom a cache buys vs. the miss path.
fn bench_highlight_hit(
    source: &str,
    lang: &gargo::syntax::language::LanguageDef,
    warmup: usize,
    iterations: usize,
) -> Vec<f64> {
    // Prime the cache once.
    let _ = highlight_text(source, lang);
    for _ in 0..warmup {
        let _ = highlight_text(source, lang);
    }

    let mut times = Vec::with_capacity(iterations);
    for _ in 0..iterations {
        let t0 = Instant::now();
        let spans = highlight_text(source, lang);
        times.push(t0.elapsed().as_secs_f64() * 1_000_000.0);
        std::hint::black_box(&spans);
    }
    times
}

// ---------------------------------------------------------------------------
// Benchmark: gix-backed read-only git helpers
// ---------------------------------------------------------------------------

async fn status_snapshot(root: &Path) {
    let (changed, staged) = git::git_status_files_in(root).unwrap_or_default();
    for entry in changed {
        let _ = git::git_diff_in(root, &entry.path, false);
    }
    for entry in staged {
        let _ = git::git_diff_in(root, &entry.path, true);
    }
}

async fn page_context(root: &Path) {
    let _branch = git::git_branch_in(root);
    let _branches = git::git_local_branches_in(root);
}

async fn commit_detail(root: &Path) {
    let _meta = git::git_show_metadata_in(root, "HEAD");
    let _files = git::git_diff_tree_in(root, "HEAD");
    let _diff = git::git_show_diff_in(root, "HEAD");
}

async fn branch_diff(root: &Path) {
    let _files = git::git_branch_diff_files_in(root, "HEAD~1");
}

/// Time an async closure over warmup + iterations, returning per-iter micros.
fn time_async<F, Fut>(
    rt: &tokio::runtime::Runtime,
    warmup: usize,
    iters: usize,
    mut f: F,
) -> Vec<f64>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    for _ in 0..warmup {
        rt.block_on(f());
    }
    let mut times = Vec::with_capacity(iters);
    for _ in 0..iters {
        let t0 = Instant::now();
        rt.block_on(f());
        times.push(t0.elapsed().as_secs_f64() * 1_000_000.0);
    }
    times
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

fn main() {
    let warmup = 5;
    let iterations = 50;
    let root = repo_root();

    println!("Gargo Benchmark: Server hot paths (release build)");
    println!("Repo: {}", root.display());
    println!("Iterations: {iterations}, Warmup: {warmup}");

    // -----------------------------------------------------------------------
    // 1. collect_files  (/api/files)
    // -----------------------------------------------------------------------
    println!();
    println!("=== collect_files: /api/files hot path (git ls-files x2) ===");
    println!("{:>8} {:>10} {:>10} {:>10}", "files", "avg", "p95", "p99");

    let (mut times, n) = bench_collect_files(&root, warmup, iterations);
    let avg = format_us(stat_avg(&times));
    let p95 = format_us(stat_percentile(&mut times, 95.0));
    let p99 = format_us(stat_percentile(&mut times, 99.0));
    println!("{n:>8} {avg:>10} {p95:>10} {p99:>10}");

    // -----------------------------------------------------------------------
    // 2. highlight_text  (/api/highlight, /blob render)
    // -----------------------------------------------------------------------
    println!();
    println!("=== highlight_text: tree-sitter highlight per request ===");
    println!(
        "{:>22} {:>6} {:>8} {:>10} {:>10} {:>10}",
        "file", "lines", "mode", "avg", "p95", "p99"
    );

    let registry = LanguageRegistry::new();
    // A handful of real source files of varying size, so the numbers map onto
    // what an editor session actually highlights.
    let samples = [
        "src/syntax/highlight.rs",
        "src/command/diff_server.rs",
        "README.md",
        "src/command/web_editor_server.rs",
    ];

    for (i, rel) in samples.iter().enumerate() {
        let path = root.join(rel);
        let Ok(source) = std::fs::read_to_string(&path) else {
            println!("{rel:>22}  (skipped: unreadable)");
            continue;
        };
        let Some(lang) = registry.detect_by_extension(rel) else {
            println!("{rel:>22}  (skipped: no language)");
            continue;
        };
        let lines = source.lines().count();

        let mut miss = bench_highlight_miss(&source, lang, warmup, iterations, i * 100_000);
        let m_avg = format_us(stat_avg(&miss));
        let m_p95 = format_us(stat_percentile(&mut miss, 95.0));
        let m_p99 = format_us(stat_percentile(&mut miss, 99.0));
        println!(
            "{:>22} {:>6} {:>8} {:>10} {:>10} {:>10}",
            rel, lines, "miss", m_avg, m_p95, m_p99
        );

        let mut hit = bench_highlight_hit(&source, lang, warmup, iterations);
        let h_avg = format_us(stat_avg(&hit));
        let h_p95 = format_us(stat_percentile(&mut hit, 95.0));
        let h_p99 = format_us(stat_percentile(&mut hit, 99.0));
        println!(
            "{:>22} {:>6} {:>8} {:>10} {:>10} {:>10}",
            "", "", "hit", h_avg, h_p95, h_p99
        );
    }

    // -----------------------------------------------------------------------
    // 3. gix-backed read-only git helpers
    // -----------------------------------------------------------------------
    println!();
    println!("=== gix read-only git helpers ===");
    println!("{:>26} {:>10} {:>10} {:>10}", "case", "avg", "p95", "p99");

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");

    let report = |label: &str, times: &mut Vec<f64>| {
        let avg = format_us(stat_avg(times));
        let p95 = format_us(stat_percentile(times, 95.0));
        let p99 = format_us(stat_percentile(times, 99.0));
        println!("{label:>26} {avg:>10} {p95:>10} {p99:>10}");
    };

    let mut t = time_async(&rt, warmup, iterations, || status_snapshot(&root));
    report("api/status snapshot", &mut t);
    let mut t = time_async(&rt, warmup, iterations, || page_context(&root));
    report("page branch context", &mut t);
    let mut t = time_async(&rt, warmup, iterations, || commit_detail(&root));
    report("api/commit detail", &mut t);
    let mut t = time_async(&rt, warmup, iterations, || branch_diff(&root));
    report("api/branch diff", &mut t);
}
