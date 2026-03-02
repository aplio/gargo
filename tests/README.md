# Gargo Test Suite

## Test Categories

### 1. Performance Tests (`render_performance_test.rs`)

Ensures that methods called every frame complete quickly enough to maintain 60 FPS.

```bash
cargo test --test render_performance_test
```

**What it tests:**
- `status_bar_path()` - Must be nearly instant (cached)
- `display_name()` - Must complete in < 16ms for 1000 calls
- Scratch buffer operations - Must be even faster

**Why:** The render loop runs at ~60 FPS with a 16ms frame budget. Slow methods will block keyboard input.

### 2. Static Analysis Tests (`no_blocking_in_hot_paths.rs`)

Scans source code for blocking operations in render paths.

```bash
cargo test --test no_blocking_in_hot_paths
```

**What it detects:**
- `Command::new()` - Process spawning
- `File::open()` - Blocking I/O
- `thread::sleep()` - Thread blocking
- Network operations
- Other blocking syscalls

**Where it looks:**
- `src/ui/views/status_bar.rs`
- `src/ui/views/notification_bar.rs`
- `src/ui/views/text_view.rs`
- `src/ui/framework/compositor.rs`
- `src/ui/overlays/command_helper.rs`

**Why:** Blocking operations in these files will freeze the editor.

### 3. Action Routing Tests (`action_routing_test.rs`)

Tests keyboard event routing and action dispatch.

```bash
cargo test --test action_routing_test
```

### 4. Render Snapshot E2E (`render_snapshot_e2e.rs`)

Headless terminal render snapshots for core editor frames.

```bash
cargo test --test render_snapshot_e2e
```

**What it tests:**
- Scratch baseline frame
- Basic multiline rendering with status/cursor info
- Insert mode frame rendering

**Fixture update mode:**
```bash
UPDATE_RENDER_FIXTURES=1 cargo test --test render_snapshot_e2e
```

## Running All Tests

```bash
# All tests
cargo test

# E2E/integration-focused script
tests/test-e2e.sh

# Just performance tests
cargo test performance

# Just blocking operation checks
cargo test blocking

# With output
cargo test -- --nocapture

# Specific test
cargo test test_status_bar_path_is_fast
```

## Adding New Tests

When adding code to hot paths:

1. **Add a performance test** if you're adding a new method called from render:
   ```rust
   #[test]
   fn test_my_new_method_is_fast() {
       let start = Instant::now();
       for _ in 0..1000 {
           my_new_method();
       }
       assert!(start.elapsed().as_millis() < 16);
   }
   ```

2. **Update `HOT_PATH_FILES`** if you create a new UI component:
   ```rust
   const HOT_PATH_FILES: &[&str] = &[
       "src/ui/views/my_new_component.rs",  // Add here
       // ...
   ];
   ```

3. **Run tests before committing:**
   ```bash
   cargo test --test render_performance_test
   cargo test --test no_blocking_in_hot_paths
   ```

## CI Integration

These tests run automatically on:
- Every pull request
- Every push to main

See `.github/workflows/performance-checks.yml`

## Troubleshooting

**Test fails with "Blocking operation detected":**
- Check the file and line number in the error
- Move the blocking operation out of the render method
- Cache the result instead of computing it every frame
- See `docs/PERFORMANCE_RULES.md` for patterns

**Test fails with "took Xms":**
- Profile the method to find the slow part
- Cache expensive computations
- Use references instead of cloning
- See if you can avoid the work entirely
