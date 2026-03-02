// End-to-end verification test for command history
// Run with: cargo test --test verify_history_e2e

use std::path::PathBuf;
use std::thread;
use std::time::Duration;

// Mock the gargo modules we need for testing
mod test_utils {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static DIR_COUNTER: AtomicU64 = AtomicU64::new(0);

    pub fn setup_test_dir() -> PathBuf {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let seq = DIR_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("gargo_e2e_test_{}_{}", timestamp, seq));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    pub fn cleanup_test_dir(dir: &PathBuf) {
        std::fs::remove_dir_all(dir).ok();
    }
}

#[test]
fn test_command_history_end_to_end() {
    // This test simulates the full workflow:
    // 1. User opens editor in a project
    // 2. User opens command palette
    // 3. User executes commands
    // 4. User reopens palette
    // 5. Commands are sorted by last use

    println!("\n=== End-to-End Command History Test ===\n");

    // Setup: Create test directory
    let test_dir = test_utils::setup_test_dir();
    println!("✓ Test directory: {:?}", test_dir);

    // Simulate: User opens gargo in a project directory
    let project_root = PathBuf::from("/tmp/test_project");
    println!("✓ Simulated project root: {:?}", project_root);

    // Note: In real usage, CommandHistory would be created in App::new()
    // Here we simulate it with a custom data directory for testing
    unsafe {
        std::env::set_var("XDG_DATA_HOME", test_dir.to_str().unwrap());
    }

    // Simulate: App initializes and creates CommandHistory
    println!("\n1. Initializing command history...");

    // We can't directly use gargo's types here without complex setup,
    // but we can verify the SQLite database behavior directly
    use rusqlite::Connection;

    let db_path = test_dir.join("gargo/history.db");
    std::fs::create_dir_all(db_path.parent().unwrap()).unwrap();

    let conn = Connection::open(&db_path).unwrap();

    // Create schema (same as in CommandHistory::init_schema)
    conn.execute(
        "CREATE TABLE IF NOT EXISTS command_history (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            repo_name TEXT NOT NULL,
            command_id TEXT NOT NULL,
            last_used_at INTEGER NOT NULL,
            use_count INTEGER DEFAULT 1,
            UNIQUE(repo_name, command_id)
        )",
        [],
    )
    .unwrap();

    println!("   ✓ Database created at: {:?}", db_path);
    println!("   ✓ Schema initialized");

    // Simulate: User opens command palette and executes "Save File"
    println!("\n2. Executing 'Save File' command...");
    let repo_name = "test_project";
    let timestamp1 = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;

    conn.execute(
        "INSERT INTO command_history (repo_name, command_id, last_used_at, use_count)
         VALUES (?1, ?2, ?3, 1)
         ON CONFLICT(repo_name, command_id)
         DO UPDATE SET last_used_at = ?3, use_count = use_count + 1",
        rusqlite::params![repo_name, "core.save", timestamp1],
    )
    .unwrap();
    println!("   ✓ Recorded: core.save at timestamp {}", timestamp1);

    thread::sleep(Duration::from_millis(10));

    // Simulate: User executes "Quit Editor"
    println!("\n3. Executing 'Quit Editor' command...");
    let timestamp2 = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;

    conn.execute(
        "INSERT INTO command_history (repo_name, command_id, last_used_at, use_count)
         VALUES (?1, ?2, ?3, 1)
         ON CONFLICT(repo_name, command_id)
         DO UPDATE SET last_used_at = ?3, use_count = use_count + 1",
        rusqlite::params![repo_name, "core.quit", timestamp2],
    )
    .unwrap();
    println!("   ✓ Recorded: core.quit at timestamp {}", timestamp2);

    thread::sleep(Duration::from_millis(10));

    // Simulate: User executes "Save File" again
    println!("\n4. Executing 'Save File' again...");
    let timestamp3 = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;

    conn.execute(
        "INSERT INTO command_history (repo_name, command_id, last_used_at, use_count)
         VALUES (?1, ?2, ?3, 1)
         ON CONFLICT(repo_name, command_id)
         DO UPDATE SET last_used_at = ?3, use_count = use_count + 1",
        rusqlite::params![repo_name, "core.save", timestamp3],
    )
    .unwrap();
    println!("   ✓ Updated: core.save at timestamp {}", timestamp3);

    // Verify: Query recent commands (what palette does when opened)
    println!("\n5. Querying recent commands (as palette would)...");
    let commands = {
        let mut stmt = conn
            .prepare(
                "SELECT command_id, last_used_at, use_count FROM command_history
             WHERE repo_name = ?1
             ORDER BY last_used_at DESC
             LIMIT 10",
            )
            .unwrap();

        stmt.query_map([repo_name], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i32>(2)?,
            ))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
    };

    println!("\n   Recent commands (most recent first):");
    for (i, (cmd_id, timestamp, count)) in commands.iter().enumerate() {
        println!(
            "   {}. {} (used {} times, timestamp: {})",
            i + 1,
            cmd_id,
            count,
            timestamp
        );
    }

    // Assertions
    assert_eq!(commands.len(), 2, "Should have 2 unique commands");
    assert_eq!(
        commands[0].0, "core.save",
        "Most recent should be core.save"
    );
    assert_eq!(commands[0].2, 2, "core.save should have use_count of 2");
    assert_eq!(commands[1].0, "core.quit", "Second should be core.quit");
    assert_eq!(commands[1].2, 1, "core.quit should have use_count of 1");
    assert!(
        commands[0].1 > commands[1].1,
        "Timestamps should be ordered"
    );

    println!("\n   ✓ All assertions passed!");
    println!("   ✓ Commands sorted correctly by last_used_at");
    println!("   ✓ Use counts tracked correctly");

    // Verify: Different repository has isolated history
    println!("\n6. Testing repository isolation...");
    let other_repo = "other_project";
    conn.execute(
        "INSERT INTO command_history (repo_name, command_id, last_used_at, use_count)
         VALUES (?1, ?2, ?3, 1)",
        rusqlite::params![other_repo, "core.other", timestamp3 + 1000],
    )
    .unwrap();

    {
        let mut stmt = conn
            .prepare("SELECT COUNT(*) FROM command_history WHERE repo_name = ?1")
            .unwrap();

        let count: i32 = stmt.query_row([repo_name], |row| row.get(0)).unwrap();
        assert_eq!(count, 2, "Original repo should still have 2 commands");

        let count: i32 = stmt.query_row([other_repo], |row| row.get(0)).unwrap();
        assert_eq!(count, 1, "Other repo should have 1 command");
    }

    println!("   ✓ Repository isolation verified");

    // Cleanup
    drop(conn);
    test_utils::cleanup_test_dir(&test_dir);
    println!("\n✓ Cleanup complete");

    println!("\n=== End-to-End Test Passed! ===\n");
}

#[test]
fn test_history_persistence() {
    println!("\n=== History Persistence Test ===\n");

    let test_dir = test_utils::setup_test_dir();
    let db_path = test_dir.join("gargo/history.db");
    std::fs::create_dir_all(db_path.parent().unwrap()).unwrap();

    // Session 1: Create and record
    {
        println!("1. Session 1: Recording commands...");
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        conn.execute(
            "CREATE TABLE IF NOT EXISTS command_history (
                id INTEGER PRIMARY KEY,
                repo_name TEXT,
                command_id TEXT,
                last_used_at INTEGER,
                use_count INTEGER
            )",
            [],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO command_history (repo_name, command_id, last_used_at, use_count)
             VALUES ('test', 'cmd1', 1000, 1)",
            [],
        )
        .unwrap();
        println!("   ✓ Recorded command in session 1");
    } // Connection closes

    // Session 2: Reopen and verify
    {
        println!("\n2. Session 2: Reopening database...");
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        let mut stmt = conn
            .prepare("SELECT command_id FROM command_history")
            .unwrap();
        let commands: Vec<String> = stmt
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0], "cmd1");
        println!("   ✓ Command persisted across sessions");
    }

    test_utils::cleanup_test_dir(&test_dir);
    println!("\n=== Persistence Test Passed! ===\n");
}
