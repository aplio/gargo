use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc;

use regex::{Regex, RegexBuilder};

use crate::syntax::highlight::highlight_text;
use crate::syntax::language::LanguageRegistry;

use super::{
    GlobalSearchBatch, GlobalSearchRequest, GlobalSearchResultEntry, PreviewRequest, PreviewResult,
};

pub(super) const GLOBAL_SEARCH_DEBOUNCE_MS: u64 = 120;
const GLOBAL_SEARCH_MAX_RESULTS: usize = 400;
const GLOBAL_SEARCH_CONTEXT_LINES: usize = 3;

#[derive(Debug)]
struct ParsedGlobalSearchRequest {
    query: String,
    include_dir: Option<String>,
    exclude_regex: Option<Regex>,
}

pub(super) fn preview_worker(
    rx: mpsc::Receiver<PreviewRequest>,
    tx: mpsc::Sender<PreviewResult>,
    project_root: PathBuf,
) {
    let lang_registry = LanguageRegistry::new();
    while let Ok(req) = rx.recv() {
        let full_path = project_root.join(&req.rel_path);
        let content = match std::fs::read_to_string(&full_path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let lines: Vec<String> = content.lines().take(200).map(|l| l.to_string()).collect();
        let spans = if let Some(lang_def) = lang_registry.detect_by_extension(&req.rel_path) {
            let preview_text: String = lines.join("\n");
            highlight_text(&preview_text, lang_def)
        } else {
            HashMap::new()
        };

        if tx
            .send(PreviewResult {
                rel_path: req.rel_path,
                lines,
                spans,
            })
            .is_err()
        {
            break;
        }
    }
}

fn format_search_preview_line(line_no: usize, line: &str) -> String {
    format!("{:>5} | {}", line_no + 1, line)
}

fn build_search_preview(lines: &[&str], match_line: usize) -> Vec<String> {
    let start = match_line.saturating_sub(GLOBAL_SEARCH_CONTEXT_LINES);
    let end = (match_line + GLOBAL_SEARCH_CONTEXT_LINES + 1).min(lines.len());
    let mut preview = Vec::new();
    for (idx, line) in lines.iter().enumerate().take(end).skip(start) {
        preview.push(format_search_preview_line(idx, line));
    }
    preview
}

pub(super) fn find_global_search_matches(
    rel_path: &str,
    content: &str,
    query: &str,
    max_results: usize,
) -> Vec<GlobalSearchResultEntry> {
    let mut results = Vec::new();
    if query.trim().is_empty() {
        return results;
    }

    let regex = match RegexBuilder::new(&regex::escape(query))
        .case_insensitive(true)
        .build()
    {
        Ok(r) => r,
        Err(_) => return results,
    };

    let lines: Vec<&str> = content.lines().collect();
    for (line_idx, line) in lines.iter().enumerate() {
        let Some(m) = regex.find(line) else {
            continue;
        };
        let char_col = line[..m.start()].chars().count();
        let preview_lines = build_search_preview(&lines, line_idx);
        results.push(GlobalSearchResultEntry {
            rel_path: rel_path.to_string(),
            line: line_idx,
            char_col,
            preview_lines: {
                let mut p = vec![format!("{}:{}:{}", rel_path, line_idx + 1, char_col + 1)];
                p.extend(preview_lines);
                p
            },
        });

        if results.len() >= max_results {
            break;
        }
    }

    results
}

fn tokenize_search_input(input: &str) -> Result<Vec<String>, String> {
    let mut tokens = Vec::new();
    let mut current = String::new();

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum QuoteState {
        None,
        Single,
        Double,
    }

    let mut state = QuoteState::None;
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        match state {
            QuoteState::None => {
                if ch.is_whitespace() {
                    if !current.is_empty() {
                        tokens.push(std::mem::take(&mut current));
                    }
                    continue;
                }
                if ch == '\'' {
                    state = QuoteState::Single;
                    continue;
                }
                if ch == '"' {
                    state = QuoteState::Double;
                    continue;
                }
                current.push(ch);
            }
            QuoteState::Single => {
                if ch == '\'' {
                    state = QuoteState::None;
                } else {
                    current.push(ch);
                }
            }
            QuoteState::Double => {
                if ch == '"' {
                    state = QuoteState::None;
                } else if ch == '\\' {
                    if let Some(next) = chars.next() {
                        current.push(next);
                    } else {
                        current.push('\\');
                    }
                } else {
                    current.push(ch);
                }
            }
        }
    }

    if state != QuoteState::None {
        return Err("Unterminated quote in global search input".to_string());
    }

    if !current.is_empty() {
        tokens.push(current);
    }

    Ok(tokens)
}

fn path_to_slash_string(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn normalize_rel_path(path: &str) -> String {
    path.replace('\\', "/")
        .trim_start_matches("./")
        .trim_start_matches('/')
        .to_string()
}

fn is_included_dir(rel_path: &str, include_dir: Option<&str>) -> bool {
    let Some(dir) = include_dir else {
        return true;
    };
    if dir.is_empty() {
        return true;
    }
    rel_path == dir || rel_path.starts_with(&format!("{dir}/"))
}

fn resolve_include_dir(
    project_root: &Path,
    include_dir_arg: Option<&str>,
) -> Result<Option<String>, String> {
    let Some(dir_arg) = include_dir_arg else {
        return Ok(None);
    };

    let root_canonical =
        std::fs::canonicalize(project_root).unwrap_or_else(|_| project_root.to_path_buf());
    let input_path = PathBuf::from(dir_arg);
    let candidate = if input_path.is_absolute() {
        input_path
    } else {
        project_root.join(input_path)
    };

    if !candidate.exists() {
        return Err(format!("--dir path does not exist: {dir_arg}"));
    }
    if !candidate.is_dir() {
        return Err(format!("--dir path is not a directory: {dir_arg}"));
    }

    let candidate_canonical = std::fs::canonicalize(&candidate).unwrap_or(candidate);
    if !candidate_canonical.starts_with(&root_canonical) {
        return Err(format!("--dir path is outside project root: {dir_arg}"));
    }

    let rel = candidate_canonical
        .strip_prefix(&root_canonical)
        .unwrap_or(Path::new(""));
    let rel = path_to_slash_string(rel)
        .trim_start_matches('/')
        .to_string();
    if rel.is_empty() {
        Ok(None)
    } else {
        Ok(Some(rel))
    }
}

fn parse_global_search_request(
    input: &str,
    project_root: &Path,
) -> Result<ParsedGlobalSearchRequest, String> {
    let tokens = tokenize_search_input(input)?;
    let mut query_tokens = Vec::new();
    let mut include_dir_arg: Option<String> = None;
    let mut exclude_arg: Option<String> = None;
    let mut i = 0usize;

    while i < tokens.len() {
        match tokens[i].as_str() {
            "--dir" => {
                let Some(value) = tokens.get(i + 1) else {
                    return Err("Missing value for --dir".to_string());
                };
                include_dir_arg = Some(value.clone());
                i += 2;
            }
            "--exclude" => {
                let Some(value) = tokens.get(i + 1) else {
                    return Err("Missing value for --exclude".to_string());
                };
                exclude_arg = Some(value.clone());
                i += 2;
            }
            _ => {
                query_tokens.push(tokens[i].clone());
                i += 1;
            }
        }
    }

    let include_dir = resolve_include_dir(project_root, include_dir_arg.as_deref())?;
    let exclude_regex = match exclude_arg {
        Some(pattern) => {
            Some(Regex::new(&pattern).map_err(|err| format!("Invalid --exclude regex: {err}"))?)
        }
        None => None,
    };

    Ok(ParsedGlobalSearchRequest {
        query: query_tokens.join(" "),
        include_dir,
        exclude_regex,
    })
}

fn execute_global_search(
    req: &GlobalSearchRequest,
    project_root: &Path,
    file_entries: &[String],
) -> GlobalSearchBatch {
    let parsed = match parse_global_search_request(&req.query, project_root) {
        Ok(parsed) => parsed,
        Err(err) => {
            return GlobalSearchBatch {
                generation: req.generation,
                results: Vec::new(),
                error: Some(err),
            };
        }
    };

    if parsed.query.trim().is_empty() {
        return GlobalSearchBatch {
            generation: req.generation,
            results: Vec::new(),
            error: None,
        };
    }

    let mut results = Vec::new();
    for rel_path in file_entries {
        let rel_path_normalized = normalize_rel_path(rel_path);
        if !is_included_dir(&rel_path_normalized, parsed.include_dir.as_deref()) {
            continue;
        }
        if parsed
            .exclude_regex
            .as_ref()
            .is_some_and(|regex| regex.is_match(&rel_path_normalized))
        {
            continue;
        }

        let full_path = project_root.join(rel_path);
        let content = match std::fs::read_to_string(&full_path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let remaining = GLOBAL_SEARCH_MAX_RESULTS.saturating_sub(results.len());
        if remaining == 0 {
            break;
        }

        let mut file_results =
            find_global_search_matches(&rel_path_normalized, &content, &parsed.query, remaining);
        results.append(&mut file_results);
        if results.len() >= GLOBAL_SEARCH_MAX_RESULTS {
            break;
        }
    }

    GlobalSearchBatch {
        generation: req.generation,
        results,
        error: None,
    }
}

pub(super) fn global_search_worker(
    rx: mpsc::Receiver<GlobalSearchRequest>,
    tx: mpsc::Sender<GlobalSearchBatch>,
    project_root: PathBuf,
    file_entries: Vec<String>,
) {
    while let Ok(mut req) = rx.recv() {
        while let Ok(next_req) = rx.try_recv() {
            req = next_req;
        }

        let batch = execute_global_search(&req, &project_root, &file_entries);
        if tx.send(batch).is_err() {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn write_file(root: &Path, rel_path: &str, content: &str) {
        let path = root.join(rel_path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, content).unwrap();
    }

    fn run_search(root: &Path, file_entries: &[&str], query: &str) -> GlobalSearchBatch {
        let req = GlobalSearchRequest {
            query: query.to_string(),
            generation: 1,
        };
        let files = file_entries
            .iter()
            .map(|p| (*p).to_string())
            .collect::<Vec<_>>();
        execute_global_search(&req, root, &files)
    }

    #[test]
    fn global_search_no_flags_behaves_like_existing_search() {
        let tmp = tempdir().unwrap();
        write_file(
            tmp.path(),
            "src/main.rs",
            "First Line\nsecond line\nTHIRD line\n",
        );

        let batch = run_search(tmp.path(), &["src/main.rs"], "line");
        assert!(batch.error.is_none());
        assert_eq!(batch.results.len(), 3);
        assert_eq!(batch.results[0].line, 0);
    }

    #[test]
    fn global_search_dir_relative_filters_scope() {
        let tmp = tempdir().unwrap();
        write_file(tmp.path(), "src/main.rs", "needle\n");
        write_file(tmp.path(), "docs/readme.md", "needle\n");

        let batch = run_search(
            tmp.path(),
            &["src/main.rs", "docs/readme.md"],
            "needle --dir src",
        );
        assert!(batch.error.is_none());
        assert_eq!(batch.results.len(), 1);
        assert_eq!(batch.results[0].rel_path, "src/main.rs");
    }

    #[test]
    fn global_search_dir_absolute_inside_root_works() {
        let tmp = tempdir().unwrap();
        let src_dir = tmp.path().join("src");
        write_file(tmp.path(), "src/main.rs", "needle\n");
        write_file(tmp.path(), "docs/readme.md", "needle\n");

        let abs_src = std::fs::canonicalize(src_dir).unwrap();
        let query = format!("needle --dir '{}'", abs_src.to_string_lossy());
        let batch = run_search(tmp.path(), &["src/main.rs", "docs/readme.md"], &query);
        assert!(batch.error.is_none());
        assert_eq!(batch.results.len(), 1);
        assert_eq!(batch.results[0].rel_path, "src/main.rs");
    }

    #[test]
    fn global_search_dir_absolute_outside_root_returns_error() {
        let project = tempdir().unwrap();
        let outside = tempdir().unwrap();
        write_file(project.path(), "src/main.rs", "needle\n");

        let query = format!("needle --dir {}", outside.path().to_string_lossy());
        let batch = run_search(project.path(), &["src/main.rs"], &query);
        assert!(batch.results.is_empty());
        assert!(
            batch
                .error
                .as_deref()
                .unwrap_or("")
                .contains("outside project root")
        );
    }

    #[test]
    fn global_search_exclude_regex_filters_paths() {
        let tmp = tempdir().unwrap();
        write_file(tmp.path(), "src/lib.rs", "needle\n");
        write_file(tmp.path(), "src/generated/out.rs", "needle\n");

        let batch = run_search(
            tmp.path(),
            &["src/lib.rs", "src/generated/out.rs"],
            "needle --exclude ^src/generated/",
        );
        assert!(batch.error.is_none());
        assert_eq!(batch.results.len(), 1);
        assert_eq!(batch.results[0].rel_path, "src/lib.rs");
    }

    #[test]
    fn global_search_invalid_exclude_regex_returns_error() {
        let tmp = tempdir().unwrap();
        write_file(tmp.path(), "src/lib.rs", "needle\n");

        let batch = run_search(tmp.path(), &["src/lib.rs"], "needle --exclude [");
        assert!(batch.results.is_empty());
        assert!(
            batch
                .error
                .as_deref()
                .unwrap_or("")
                .contains("Invalid --exclude regex")
        );
    }

    #[test]
    fn global_search_combined_dir_and_exclude_filters() {
        let tmp = tempdir().unwrap();
        write_file(tmp.path(), "src/lib.rs", "needle\n");
        write_file(tmp.path(), "src/generated/out.rs", "needle\n");
        write_file(tmp.path(), "docs/readme.md", "needle\n");

        let batch = run_search(
            tmp.path(),
            &["src/lib.rs", "src/generated/out.rs", "docs/readme.md"],
            "needle --dir src --exclude generated",
        );
        assert!(batch.error.is_none());
        assert_eq!(batch.results.len(), 1);
        assert_eq!(batch.results[0].rel_path, "src/lib.rs");
    }

    #[test]
    fn global_search_duplicate_flags_use_rightmost_value() {
        let tmp = tempdir().unwrap();
        write_file(tmp.path(), "src/lib.rs", "needle\n");
        write_file(tmp.path(), "docs/readme.md", "needle\n");

        let batch = run_search(
            tmp.path(),
            &["src/lib.rs", "docs/readme.md"],
            "needle --dir docs --dir src",
        );
        assert!(batch.error.is_none());
        assert_eq!(batch.results.len(), 1);
        assert_eq!(batch.results[0].rel_path, "src/lib.rs");
    }

    #[test]
    fn global_search_unknown_flags_are_treated_as_query_text() {
        let tmp = tempdir().unwrap();
        write_file(tmp.path(), "src/lib.rs", "--weird needle\n");

        let batch = run_search(tmp.path(), &["src/lib.rs"], "--weird needle");
        assert!(batch.error.is_none());
        assert_eq!(batch.results.len(), 1);
    }
}
