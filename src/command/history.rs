use rusqlite::{Connection, Result};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub struct CommandHistory {
    conn: Option<Connection>,
    repo_name: String,
}

impl CommandHistory {
    /// Initialize command history database
    pub fn new(project_root: &Path) -> Self {
        let repo_name = Self::extract_repo_name(project_root);
        let conn = Self::init_db(None).ok();

        Self { conn, repo_name }
    }

    /// Initialize SQLite database with schema
    fn init_db(custom_data_dir: Option<PathBuf>) -> Result<Connection> {
        let data_dir = custom_data_dir.unwrap_or_else(Self::get_data_dir);
        std::fs::create_dir_all(&data_dir).ok();

        let db_path = data_dir.join("history.db");
        let conn = Connection::open(db_path)?;

        Self::init_schema(&conn)?;

        Ok(conn)
    }

    #[cfg(test)]
    pub fn new_with_data_dir(project_root: &Path, data_dir: PathBuf) -> Self {
        let repo_name = Self::extract_repo_name(project_root);
        let conn = Self::init_db(Some(data_dir)).ok();

        Self { conn, repo_name }
    }

    /// Create database schema
    fn init_schema(conn: &Connection) -> Result<()> {
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
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_repo_command
             ON command_history(repo_name, command_id)",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_repo_last_used
             ON command_history(repo_name, last_used_at DESC)",
            [],
        )?;

        Ok(())
    }

    /// Get XDG_DATA_HOME or fallback to ~/.local/share
    fn get_data_dir() -> PathBuf {
        if let Ok(xdg_data) = std::env::var("XDG_DATA_HOME") {
            PathBuf::from(xdg_data).join("gargo")
        } else if let Ok(home) = std::env::var("HOME") {
            PathBuf::from(home).join(".local/share/gargo")
        } else {
            PathBuf::from(".gargo")
        }
    }

    /// Extract repository name from git remote or use directory name
    fn extract_repo_name(project_root: &Path) -> String {
        // Try to get git remote URL
        if let Ok(output) = std::process::Command::new("git")
            .arg("remote")
            .arg("get-url")
            .arg("origin")
            .current_dir(project_root)
            .output()
            && output.status.success()
            && let Ok(url) = String::from_utf8(output.stdout)
        {
            let url = url.trim();
            // Parse owner/repo from URL
            // Examples:
            // - git@github.com:aplio/gargo.git -> aplio/gargo
            // - https://github.com/aplio/gargo.git -> aplio/gargo
            if let Some(repo) = Self::parse_repo_from_url(url) {
                return repo;
            }
        }

        // Fallback to directory name
        project_root
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string()
    }

    /// Parse repository name from git URL
    fn parse_repo_from_url(url: &str) -> Option<String> {
        // Handle SSH URLs: git@github.com:owner/repo.git
        if url.starts_with("git@")
            && let Some(colon_pos) = url.find(':')
        {
            let path = &url[colon_pos + 1..];
            let path = path.trim_end_matches(".git");
            return Some(path.to_string());
        }

        // Handle HTTPS URLs: https://github.com/owner/repo.git
        if url.starts_with("http://") || url.starts_with("https://") {
            let path = url
                .trim_start_matches("https://")
                .trim_start_matches("http://");

            if let Some(slash_pos) = path.find('/') {
                let path = &path[slash_pos + 1..];
                let path = path.trim_end_matches(".git");
                return Some(path.to_string());
            }
        }

        None
    }

    /// Record command execution (UPSERT)
    pub fn record_execution(&self, command_id: &str) -> Result<()> {
        let conn = match &self.conn {
            Some(c) => c,
            None => return Ok(()), // Silently skip if no DB connection
        };

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;

        conn.execute(
            "INSERT INTO command_history (repo_name, command_id, last_used_at, use_count)
             VALUES (?1, ?2, ?3, 1)
             ON CONFLICT(repo_name, command_id)
             DO UPDATE SET
                last_used_at = ?3,
                use_count = use_count + 1",
            rusqlite::params![&self.repo_name, command_id, timestamp],
        )?;

        Ok(())
    }

    /// Get recent commands sorted by last used time
    pub fn get_recent_commands(&self, limit: usize) -> Vec<String> {
        let conn = match &self.conn {
            Some(c) => c,
            None => return Vec::new(),
        };

        let mut stmt = match conn.prepare(
            "SELECT command_id FROM command_history
             WHERE repo_name = ?1
             ORDER BY last_used_at DESC
             LIMIT ?2",
        ) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };

        let rows = stmt.query_map(rusqlite::params![&self.repo_name, limit as i64], |row| {
            row.get::<_, String>(0)
        });

        match rows {
            Ok(rows) => rows.filter_map(|r| r.ok()).collect(),
            Err(_) => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_parse_repo_from_url_ssh() {
        assert_eq!(
            CommandHistory::parse_repo_from_url("git@github.com:aplio/gargo.git"),
            Some("aplio/gargo".to_string())
        );
    }

    #[test]
    fn test_parse_repo_from_url_https() {
        assert_eq!(
            CommandHistory::parse_repo_from_url("https://github.com/aplio/gargo.git"),
            Some("aplio/gargo".to_string())
        );
    }

    #[test]
    fn test_parse_repo_from_url_invalid() {
        assert_eq!(CommandHistory::parse_repo_from_url("invalid"), None);
    }

    #[test]
    fn test_record_and_retrieve() {
        use std::time::SystemTime;
        let timestamp = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let temp_dir = std::env::temp_dir().join(format!("gargo_test_history_{}", timestamp));
        std::fs::create_dir_all(&temp_dir).unwrap();

        let history =
            CommandHistory::new_with_data_dir(&PathBuf::from("/tmp/test_repo_1"), temp_dir.clone());

        // Record some commands
        history.record_execution("core.save").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
        history.record_execution("core.quit").unwrap();

        // Retrieve recent commands
        let recent = history.get_recent_commands(10);
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0], "core.quit"); // Most recent first
        assert_eq!(recent[1], "core.save");

        // Clean up
        std::fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_update_timestamp_on_reuse() {
        use std::time::SystemTime;
        let timestamp = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let temp_dir = std::env::temp_dir().join(format!("gargo_test_reuse_{}", timestamp));
        std::fs::create_dir_all(&temp_dir).unwrap();

        let history =
            CommandHistory::new_with_data_dir(&PathBuf::from("/tmp/test_repo_2"), temp_dir.clone());

        history.record_execution("core.save").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
        history.record_execution("core.quit").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
        history.record_execution("core.save").unwrap(); // Re-use

        let recent = history.get_recent_commands(10);

        assert_eq!(recent[0], "core.save"); // Should be most recent now

        std::fs::remove_dir_all(&temp_dir).ok();
    }
}
