/// Tests to ensure render methods don't block the event loop
///
/// The event loop runs at ~60 FPS with a 16ms frame budget.
/// Render methods must complete in < 16ms to avoid blocking keyboard input.
use gargo::core::document::Document;
use std::time::Instant;

const FRAME_BUDGET_MS: u128 = 16;

#[test]
fn test_status_bar_path_is_fast() {
    // Create a file-backed document
    let doc = Document::from_file(1, "src/main.rs");

    let start = Instant::now();
    let iterations = 1000; // Simulate many frames

    for _ in 0..iterations {
        let _ = doc.status_bar_path();
    }

    let elapsed = start.elapsed().as_millis();
    let avg_per_call = elapsed as f64 / iterations as f64;

    // Each call should be virtually instant (< 1ms for 1000 calls)
    assert!(
        elapsed < FRAME_BUDGET_MS,
        "status_bar_path took {}ms for {} calls (avg {:.3}ms/call), must complete in < {}ms total",
        elapsed,
        iterations,
        avg_per_call,
        FRAME_BUDGET_MS
    );
}

#[test]
fn test_display_name_is_fast() {
    let doc = Document::from_file(1, "src/main.rs");

    let start = Instant::now();
    let iterations = 1000;

    for _ in 0..iterations {
        let _ = doc.display_name();
    }

    let elapsed = start.elapsed().as_millis();

    assert!(
        elapsed < FRAME_BUDGET_MS,
        "display_name took {}ms for {} iterations, must complete in < {}ms",
        elapsed,
        iterations,
        FRAME_BUDGET_MS
    );
}

#[test]
fn test_scratch_buffer_status_bar_path_is_fast() {
    let doc = Document::new_scratch(1);

    let start = Instant::now();
    let iterations = 10000; // Even more iterations for scratch

    for _ in 0..iterations {
        let _ = doc.status_bar_path();
    }

    let elapsed = start.elapsed().as_millis();

    assert!(
        elapsed < FRAME_BUDGET_MS,
        "scratch status_bar_path took {}ms for {} iterations, must complete in < {}ms",
        elapsed,
        iterations,
        FRAME_BUDGET_MS
    );
}
