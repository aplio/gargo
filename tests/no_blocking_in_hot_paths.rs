/// Static analysis test to detect blocking operations in hot code paths
///
/// This test scans source files for patterns that indicate blocking operations
/// in methods that are called every frame (render, status bar, etc.)
use std::fs;
use std::path::Path;

/// Blocking operations that should never be in hot paths
const BLOCKING_PATTERNS: &[(&str, &str)] = &[
    ("Command::new", "spawning subprocess"),
    (".output()", "blocking command execution"),
    (".wait()", "blocking on process"),
    ("thread::sleep", "sleeping thread"),
    ("std::io::stdin().read", "blocking I/O"),
    ("File::open", "blocking file I/O"),
    ("fs::read_to_string", "blocking file read"),
    ("TcpStream::connect", "blocking network I/O"),
];

/// Files/methods that are in the hot path (called every frame)
const HOT_PATH_FILES: &[&str] = &[
    "src/ui/views/status_bar.rs",
    "src/ui/views/notification_bar.rs",
    "src/ui/views/text_view.rs",
    "src/ui/framework/compositor/rendering.rs",
    "src/ui/overlays/command_helper.rs",
];

/// Methods that are called frequently and must not block
const HOT_PATH_METHODS: &[&str] = &[
    "fn render(",
    "fn status_bar_path(",
    "fn display_name(",
    "fn render_overlay(",
];

#[test]
fn test_no_blocking_in_render_methods() {
    let mut violations = Vec::new();

    for file_path in HOT_PATH_FILES {
        let path = Path::new(file_path);
        if !path.exists() {
            continue;
        }

        let content =
            fs::read_to_string(path).unwrap_or_else(|_| panic!("Failed to read {}", file_path));

        // Check if this file contains hot path methods
        let mut in_hot_method = false;
        let mut brace_depth = 0;

        for (line_num, line) in content.lines().enumerate() {
            let line_num = line_num + 1;

            // Check if we're entering a hot path method
            for method in HOT_PATH_METHODS {
                if line.contains(method) {
                    in_hot_method = true;
                    brace_depth = 0;
                }
            }

            // Track brace depth to know when method ends
            if in_hot_method {
                brace_depth += line.matches('{').count() as i32;
                brace_depth -= line.matches('}').count() as i32;

                // Check for blocking operations
                for (pattern, description) in BLOCKING_PATTERNS {
                    if line.contains(pattern) {
                        // Allow if it's in a comment
                        let trimmed = line.trim();
                        if trimmed.starts_with("//") || trimmed.starts_with("*") {
                            continue;
                        }

                        violations.push(format!(
                            "{}:{} - Blocking operation detected: {} ({})",
                            file_path, line_num, pattern, description
                        ));
                    }
                }

                // Exit method when braces are balanced
                if brace_depth == 0 && line.contains('}') {
                    in_hot_method = false;
                }
            }
        }
    }

    if !violations.is_empty() {
        panic!(
            "\n❌ Blocking operations found in hot paths:\n{}\n\n\
            Hot paths (render methods, status bar) are called ~60 times per second.\n\
            Blocking operations will freeze keyboard input and make the editor unresponsive.\n\
            \n\
            Solutions:\n\
            1. Cache the result and compute once (not every frame)\n\
            2. Move to async background task\n\
            3. Remove the blocking operation\n",
            violations.join("\n")
        );
    }
}

#[test]
fn test_status_bar_path_is_cached() {
    // Verify that status_bar_path returns a reference (cached) not owned String (computed)
    let doc_rs =
        fs::read_to_string("src/core/document/display.rs").expect("Failed to read document display.rs");

    // Find the status_bar_path method
    let method_found = doc_rs.contains("pub fn status_bar_path(&self) -> &str");

    assert!(
        method_found,
        "status_bar_path should return &str (cached value), not String (computed value). \
        This ensures it doesn't recompute on every call."
    );
}
