use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crate::syntax::language::LanguageRegistry;
use crate::syntax::symbol::extract_definition_sections;

/// Files larger than this are skipped, matching global_search_index.
const MAX_INDEX_FILE_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolLocation {
    pub rel_path: String,
    pub line: usize,
    pub char_col: usize,
    pub kind: String,
}

#[derive(Default)]
struct SymbolIndexState {
    by_name: HashMap<String, Vec<SymbolLocation>>,
    /// rel_path -> names defined there, so a re-index of one file can drop
    /// its stale entries without scanning every bucket.
    file_names: HashMap<String, Vec<String>>,
    ready: bool,
}

/// Workspace-wide, name-keyed index of tree-sitter tag definitions. Built on a
/// background thread; lookups are exact-name and may include false positives —
/// callers rank and present candidates instead of trusting a single answer.
#[derive(Debug)]
pub struct SymbolIndex {
    root: PathBuf,
    state: Mutex<SymbolIndexState>,
    refreshing: AtomicBool,
    rerun_requested: AtomicBool,
}

impl std::fmt::Debug for SymbolIndexState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SymbolIndexState")
            .field("names", &self.by_name.len())
            .field("files", &self.file_names.len())
            .field("ready", &self.ready)
            .finish()
    }
}

impl SymbolIndex {
    pub fn new(root: PathBuf) -> Arc<Self> {
        Arc::new(Self {
            root,
            state: Mutex::new(SymbolIndexState::default()),
            refreshing: AtomicBool::new(false),
            rerun_requested: AtomicBool::new(false),
        })
    }

    pub fn is_ready(&self) -> bool {
        self.state.lock().unwrap().ready
    }

    pub fn lookup(&self, name: &str) -> Vec<SymbolLocation> {
        self.state
            .lock()
            .unwrap()
            .by_name
            .get(name)
            .cloned()
            .unwrap_or_default()
    }

    /// Kick off (or coalesce into) a full background rescan of the workspace.
    pub fn request_refresh(self: &Arc<Self>) {
        self.rerun_requested.store(true, Ordering::Release);
        if self
            .refreshing
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }

        let index = Arc::clone(self);
        let _ = std::thread::Builder::new()
            .name("gargo-symbol-index".to_string())
            .spawn(move || index.refresh_loop());
    }

    /// Re-extract a single file on a short-lived background thread, replacing
    /// its bucket in the index. Used on save.
    pub fn update_file(self: &Arc<Self>, rel_path: String) {
        let index = Arc::clone(self);
        let _ = std::thread::Builder::new()
            .name("gargo-symbol-index-file".to_string())
            .spawn(move || {
                let registry = LanguageRegistry::new();
                let entries = index.extract_file(&registry, &rel_path);
                index.replace_file(&rel_path, entries);
            });
    }

    fn refresh_loop(self: Arc<Self>) {
        loop {
            self.rerun_requested.store(false, Ordering::Release);
            self.refresh_once();
            if !self.rerun_requested.swap(false, Ordering::AcqRel) {
                self.refreshing.store(false, Ordering::Release);
                if !self.rerun_requested.load(Ordering::Acquire) {
                    break;
                }
                if self
                    .refreshing
                    .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                    .is_err()
                {
                    break;
                }
            }
        }
    }

    fn refresh_once(&self) {
        let registry = LanguageRegistry::new();
        let files = crate::project::collect_files(&self.root);

        let mut by_name: HashMap<String, Vec<SymbolLocation>> = HashMap::new();
        let mut file_names: HashMap<String, Vec<String>> = HashMap::new();
        for rel_path in files {
            let entries = self.extract_file(&registry, &rel_path);
            if entries.is_empty() {
                continue;
            }
            let names = file_names.entry(rel_path).or_default();
            for (name, location) in entries {
                names.push(name.clone());
                by_name.entry(name).or_default().push(location);
            }
        }

        // Swap in the finished index in one step so lookups never observe a
        // half-built state during a rebuild.
        let mut state = self.state.lock().unwrap();
        state.by_name = by_name;
        state.file_names = file_names;
        state.ready = true;
    }

    fn extract_file(
        &self,
        registry: &LanguageRegistry,
        rel_path: &str,
    ) -> Vec<(String, SymbolLocation)> {
        let Some(lang_def) = registry.detect_by_extension(rel_path) else {
            return Vec::new();
        };
        // Markdown tags are headings/code-block pseudo-names, never identifier
        // definitions; markdown goto-def is handled by the LSP plugin builtin.
        if lang_def.tags_query.is_none() || lang_def.name == "Markdown" {
            return Vec::new();
        }

        let abs_path = self.root.join(rel_path);
        match std::fs::metadata(&abs_path) {
            Ok(meta) if meta.len() <= MAX_INDEX_FILE_BYTES => {}
            _ => return Vec::new(),
        }
        let Ok(text) = std::fs::read_to_string(&abs_path) else {
            return Vec::new();
        };
        if text.contains('\0') {
            return Vec::new();
        }

        extract_definition_sections(&text, lang_def)
            .into_iter()
            .map(|section| {
                (
                    section.name.clone(),
                    SymbolLocation {
                        rel_path: rel_path.to_string(),
                        line: section.line,
                        char_col: section.char_col,
                        kind: section.kind,
                    },
                )
            })
            .collect()
    }

    fn replace_file(&self, rel_path: &str, entries: Vec<(String, SymbolLocation)>) {
        let mut state = self.state.lock().unwrap();
        if let Some(old_names) = state.file_names.remove(rel_path) {
            for name in old_names {
                if let Some(bucket) = state.by_name.get_mut(&name) {
                    bucket.retain(|location| location.rel_path != rel_path);
                    if bucket.is_empty() {
                        state.by_name.remove(&name);
                    }
                }
            }
        }
        if entries.is_empty() {
            return;
        }
        let names = state.file_names.entry(rel_path.to_string()).or_default();
        let mut new_names = Vec::with_capacity(entries.len());
        for (name, _) in &entries {
            new_names.push(name.clone());
        }
        *names = new_names;
        for (name, location) in entries {
            state.by_name.entry(name).or_default().push(location);
        }
    }
}

/// Rank candidate definitions for presentation: same file first, then same
/// directory, then the rest; ties broken by path then line.
pub fn rank_symbol_hits(hits: &mut [SymbolLocation], current_rel_path: Option<&str>) {
    let current_dir = current_rel_path.and_then(parent_dir);
    hits.sort_by(|a, b| {
        let bucket_a = rank_bucket(a, current_rel_path, current_dir);
        let bucket_b = rank_bucket(b, current_rel_path, current_dir);
        bucket_a
            .cmp(&bucket_b)
            .then_with(|| a.rel_path.cmp(&b.rel_path))
            .then_with(|| a.line.cmp(&b.line))
    });
}

fn rank_bucket(
    hit: &SymbolLocation,
    current_rel_path: Option<&str>,
    current_dir: Option<&str>,
) -> u8 {
    if current_rel_path == Some(hit.rel_path.as_str()) {
        return 0;
    }
    if current_dir.is_some() && parent_dir(&hit.rel_path) == current_dir {
        return 1;
    }
    2
}

fn parent_dir(rel_path: &str) -> Option<&str> {
    Some(rel_path.rsplit_once('/').map_or("", |(dir, _)| dir))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};
    use tempfile::tempdir;

    fn wait_ready(index: &Arc<SymbolIndex>) {
        let deadline = Instant::now() + Duration::from_secs(15);
        while !index.is_ready() {
            assert!(Instant::now() < deadline, "symbol index never became ready");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn loc(rel_path: &str, line: usize) -> SymbolLocation {
        SymbolLocation {
            rel_path: rel_path.to_string(),
            line,
            char_col: 0,
            kind: "function".to_string(),
        }
    }

    #[test]
    fn indexes_definitions_across_files() {
        let tmp = tempdir().unwrap();
        std::fs::write(tmp.path().join("a.rs"), "fn main() { helper(); }\n").unwrap();
        std::fs::write(tmp.path().join("b.rs"), "fn helper() {}\n").unwrap();

        let index = SymbolIndex::new(tmp.path().to_path_buf());
        index.request_refresh();
        wait_ready(&index);

        let hits = index.lookup("helper");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].rel_path, "b.rs");
        assert_eq!(hits[0].line, 0);
        assert_eq!(hits[0].kind, "function");
        // The call site in a.rs must not be indexed as a definition.
        assert!(index.lookup("main").iter().all(|h| h.rel_path == "a.rs"));
    }

    #[test]
    fn update_file_replaces_stale_entries() {
        let tmp = tempdir().unwrap();
        let file = tmp.path().join("a.rs");
        std::fs::write(&file, "fn old_name() {}\n").unwrap();

        let index = SymbolIndex::new(tmp.path().to_path_buf());
        index.request_refresh();
        wait_ready(&index);
        assert_eq!(index.lookup("old_name").len(), 1);

        std::fs::write(&file, "fn new_name() {}\n").unwrap();
        index.update_file("a.rs".to_string());

        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            if index.lookup("old_name").is_empty() && index.lookup("new_name").len() == 1 {
                break;
            }
            assert!(Instant::now() < deadline, "update_file never landed");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn skips_unindexable_files() {
        let tmp = tempdir().unwrap();
        std::fs::write(tmp.path().join("keep.rs"), "fn keep() {}\n").unwrap();
        // Markdown: excluded from the index by design.
        std::fs::write(tmp.path().join("notes.md"), "# heading\n").unwrap();
        // Unknown extension.
        std::fs::write(tmp.path().join("data.xyz"), "fn nope() {}\n").unwrap();
        // Binary content (NUL byte) with an indexable extension.
        std::fs::write(tmp.path().join("bin.rs"), b"fn bin() {}\0\n").unwrap();
        // Oversized file.
        let mut big = String::from("fn big() {}\n");
        big.push_str(&" ".repeat(MAX_INDEX_FILE_BYTES as usize + 1));
        std::fs::write(tmp.path().join("big.rs"), big).unwrap();

        let index = SymbolIndex::new(tmp.path().to_path_buf());
        index.request_refresh();
        wait_ready(&index);

        assert_eq!(index.lookup("keep").len(), 1);
        assert!(index.lookup("heading").is_empty());
        assert!(index.lookup("nope").is_empty());
        assert!(index.lookup("bin").is_empty());
        assert!(index.lookup("big").is_empty());
    }

    #[test]
    fn rank_orders_same_file_then_same_dir_then_rest() {
        let mut hits = vec![
            loc("other/far.rs", 3),
            loc("src/near.rs", 7),
            loc("src/current.rs", 20),
            loc("src/current.rs", 5),
        ];
        rank_symbol_hits(&mut hits, Some("src/current.rs"));
        assert_eq!(
            hits,
            vec![
                loc("src/current.rs", 5),
                loc("src/current.rs", 20),
                loc("src/near.rs", 7),
                loc("other/far.rs", 3),
            ]
        );
    }

    #[test]
    fn rank_without_current_path_sorts_by_path_and_line() {
        let mut hits = vec![loc("b.rs", 1), loc("a.rs", 9), loc("a.rs", 2)];
        rank_symbol_hits(&mut hits, None);
        assert_eq!(hits, vec![loc("a.rs", 2), loc("a.rs", 9), loc("b.rs", 1)]);
    }

    #[test]
    fn rank_treats_root_files_as_same_directory() {
        let mut hits = vec![loc("sub/x.rs", 1), loc("rooted.rs", 1)];
        rank_symbol_hits(&mut hits, Some("main.rs"));
        assert_eq!(hits, vec![loc("rooted.rs", 1), loc("sub/x.rs", 1)]);
    }
}
