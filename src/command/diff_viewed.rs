//! On-disk persistence for the diff server's "Viewed" file checkboxes.
//!
//! The diff server's status (`/diff`) and compare (`/compare`) pages let the
//! user tick a per-file "Viewed" checkbox. That state used to live only in the
//! browser's `localStorage`; this store moves it into gargo's data dir so it
//! survives across sessions and browsers.
//!
//! Each record stores a content hash captured when the file was marked viewed.
//! A file is only reported as viewed while that hash still matches the current
//! diff content, and — for the compare page — while the same base/compare
//! branch pair is selected. Concurrent gargo instances are not synchronised;
//! last write wins.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, Result};

/// `page` value for the git status page (`/diff`).
pub const PAGE_STATUS: &str = "status";
/// `page` value for the compare-branches page (`/compare`).
pub const PAGE_COMPARE: &str = "compare";

/// SQLite-backed store of viewed-file records.
///
/// Modelled on [`crate::command::recent_projects::RecentProjectsStore`]: if the
/// database cannot be opened the store degrades to a silent no-op so the diff
/// server keeps working without persistence.
pub struct ViewedStore {
    conn: Mutex<Option<Connection>>,
}

impl Default for ViewedStore {
    fn default() -> Self {
        Self::open()
    }
}

impl ViewedStore {
    /// Open the shared store under gargo's data dir (`~/.local/share/gargo`).
    /// Never fails: any error yields a no-op store.
    pub fn open() -> Self {
        Self::open_in_dir(&crate::config::app_data_dir())
    }

    /// Open the store under an explicit data dir. Used by tests for isolation.
    pub fn open_in_dir(data_dir: &Path) -> Self {
        Self {
            conn: Mutex::new(Self::init(data_dir).ok()),
        }
    }

    fn init(data_dir: &Path) -> Result<Connection> {
        std::fs::create_dir_all(data_dir).ok();
        let conn = Connection::open(data_dir.join("diff_viewed.db"))?;
        // Let concurrent gargo instances retry briefly instead of erroring out
        // when another process holds the write lock.
        conn.busy_timeout(Duration::from_millis(3000))?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS viewed_files (
                repo_root    TEXT NOT NULL,
                page         TEXT NOT NULL,
                base_ref     TEXT NOT NULL,
                compare_ref  TEXT NOT NULL,
                section      TEXT NOT NULL,
                path         TEXT NOT NULL,
                content_hash TEXT NOT NULL,
                updated_at   INTEGER NOT NULL,
                PRIMARY KEY (repo_root, page, base_ref, compare_ref, section, path)
            )",
            [],
        )?;
        Ok(conn)
    }

    fn now_millis() -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64
    }

    /// Every viewed record for one page / branch context, keyed by
    /// `(section, path)` and mapping to the stored content hash.
    ///
    /// For the status page `base_ref` and `compare_ref` are empty and `section`
    /// is one of `staged` / `unstaged` / `untracked`. For the compare page
    /// `section` is empty and the refs carry the selected branch pair.
    pub fn viewed_map(
        &self,
        repo_root: &str,
        page: &str,
        base_ref: &str,
        compare_ref: &str,
    ) -> HashMap<(String, String), String> {
        let Ok(guard) = self.conn.lock() else {
            return HashMap::new();
        };
        let Some(conn) = guard.as_ref() else {
            return HashMap::new();
        };
        let mut stmt = match conn.prepare(
            "SELECT section, path, content_hash FROM viewed_files
             WHERE repo_root = ?1 AND page = ?2 AND base_ref = ?3 AND compare_ref = ?4",
        ) {
            Ok(stmt) => stmt,
            Err(_) => return HashMap::new(),
        };
        let rows = stmt.query_map(
            rusqlite::params![repo_root, page, base_ref, compare_ref],
            |row| {
                Ok((
                    (row.get::<_, String>(0)?, row.get::<_, String>(1)?),
                    row.get::<_, String>(2)?,
                ))
            },
        );
        match rows {
            Ok(rows) => rows.filter_map(|r| r.ok()).collect(),
            Err(_) => HashMap::new(),
        }
    }

    /// Mark a file viewed, recording (or replacing) its content hash.
    #[allow(clippy::too_many_arguments)]
    pub fn set(
        &self,
        repo_root: &str,
        page: &str,
        base_ref: &str,
        compare_ref: &str,
        section: &str,
        path: &str,
        content_hash: &str,
    ) -> Result<()> {
        let Ok(guard) = self.conn.lock() else {
            return Ok(());
        };
        let Some(conn) = guard.as_ref() else {
            return Ok(());
        };
        conn.execute(
            "INSERT INTO viewed_files
                (repo_root, page, base_ref, compare_ref, section, path, content_hash, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(repo_root, page, base_ref, compare_ref, section, path)
             DO UPDATE SET content_hash = ?7, updated_at = ?8",
            rusqlite::params![
                repo_root,
                page,
                base_ref,
                compare_ref,
                section,
                path,
                content_hash,
                Self::now_millis(),
            ],
        )?;
        Ok(())
    }

    /// Clear a file's viewed record, if any.
    pub fn unset(
        &self,
        repo_root: &str,
        page: &str,
        base_ref: &str,
        compare_ref: &str,
        section: &str,
        path: &str,
    ) -> Result<()> {
        let Ok(guard) = self.conn.lock() else {
            return Ok(());
        };
        let Some(conn) = guard.as_ref() else {
            return Ok(());
        };
        conn.execute(
            "DELETE FROM viewed_files
             WHERE repo_root = ?1 AND page = ?2 AND base_ref = ?3
               AND compare_ref = ?4 AND section = ?5 AND path = ?6",
            rusqlite::params![repo_root, page, base_ref, compare_ref, section, path],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store() -> (tempfile::TempDir, ViewedStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = ViewedStore::open_in_dir(dir.path());
        (dir, store)
    }

    #[test]
    fn set_then_viewed_map_roundtrip() {
        let (_dir, store) = temp_store();
        store
            .set("/repo", PAGE_STATUS, "", "", "staged", "src/a.rs", "hash1")
            .unwrap();

        let map = store.viewed_map("/repo", PAGE_STATUS, "", "");
        assert_eq!(
            map.get(&("staged".to_string(), "src/a.rs".to_string())),
            Some(&"hash1".to_string()),
        );
    }

    #[test]
    fn unset_removes_record() {
        let (_dir, store) = temp_store();
        store
            .set("/repo", PAGE_STATUS, "", "", "staged", "src/a.rs", "hash1")
            .unwrap();
        store
            .unset("/repo", PAGE_STATUS, "", "", "staged", "src/a.rs")
            .unwrap();

        assert!(store.viewed_map("/repo", PAGE_STATUS, "", "").is_empty());
    }

    #[test]
    fn set_replaces_existing_hash() {
        let (_dir, store) = temp_store();
        store
            .set("/repo", PAGE_STATUS, "", "", "staged", "src/a.rs", "hash1")
            .unwrap();
        store
            .set("/repo", PAGE_STATUS, "", "", "staged", "src/a.rs", "hash2")
            .unwrap();

        let map = store.viewed_map("/repo", PAGE_STATUS, "", "");
        assert_eq!(map.len(), 1);
        assert_eq!(
            map.get(&("staged".to_string(), "src/a.rs".to_string())),
            Some(&"hash2".to_string()),
        );
    }

    #[test]
    fn status_and_compare_contexts_are_isolated() {
        let (_dir, store) = temp_store();
        store
            .set("/repo", PAGE_STATUS, "", "", "staged", "a.rs", "h")
            .unwrap();
        store
            .set("/repo", PAGE_COMPARE, "main", "dev", "", "a.rs", "h")
            .unwrap();

        assert_eq!(store.viewed_map("/repo", PAGE_STATUS, "", "").len(), 1);
        assert_eq!(
            store.viewed_map("/repo", PAGE_COMPARE, "main", "dev").len(),
            1,
        );
        // A different branch pair sees nothing.
        assert!(
            store
                .viewed_map("/repo", PAGE_COMPARE, "main", "other")
                .is_empty()
        );
    }

    #[test]
    fn noop_store_degrades_gracefully() {
        // A path that cannot be a directory yields a no-op store.
        let file = tempfile::NamedTempFile::new().unwrap();
        let store = ViewedStore::open_in_dir(file.path());
        assert!(store.viewed_map("/repo", PAGE_STATUS, "", "").is_empty());
        assert!(
            store
                .set("/repo", PAGE_STATUS, "", "", "staged", "a.rs", "h")
                .is_ok()
        );
        assert!(
            store
                .unset("/repo", PAGE_STATUS, "", "", "staged", "a.rs")
                .is_ok()
        );
    }
}
