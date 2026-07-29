use super::*;

use crate::core::buffer::BufferId;

use super::click::screen_to_doc_pos;

/// What a ctrl+click on buffer text resolved to.
#[derive(Debug, Clone, PartialEq, Eq)]
enum OpenClickTarget {
    Dir(PathBuf),
    File { path: PathBuf, line: Option<usize> },
    Url(String),
}

impl App {
    /// Open whatever sits under a ctrl+click (ghostty-style cmd+click): an
    /// existing file opens as a buffer (jumping to a trailing `:line`), an
    /// existing directory opens the explorer sidebar there, and a web URL
    /// opens in the browser. Relative paths resolve against the clicked
    /// buffer's directory, then the project root.
    pub(super) fn handle_buffer_open_click(
        &mut self,
        buffer_id: BufferId,
        screen_col: u16,
        screen_row: u16,
    ) {
        let cols = self.last_term_cols;
        let rows = self.last_term_rows;
        let Some(pane) = self.compositor.pane_at(screen_col, screen_row, cols, rows) else {
            return;
        };
        if pane.buffer_id != buffer_id {
            return;
        }
        if self.editor.active_buffer().id != buffer_id && !self.editor.switch_to_buffer(buffer_id) {
            return;
        }

        let Some(target) = screen_to_doc_pos(
            self.editor.active_buffer(),
            pane.rect,
            screen_col,
            screen_row,
            self.config.show_line_number,
            self.config.line_number_width,
        ) else {
            return;
        };
        if target.on_gutter {
            return;
        }

        let (line_text, byte_offset) = {
            let doc = self.editor.active_buffer();
            if doc.rope.len_lines() == 0 {
                return;
            }
            let raw = doc.rope.line(target.line).to_string();
            let line_text = raw.trim_end_matches(['\n', '\r']).to_string();
            let char_in_line = target
                .char_pos
                .saturating_sub(doc.rope.line_to_char(target.line));
            let Some((byte_offset, _)) = line_text.char_indices().nth(char_in_line) else {
                // Click past the end of the line.
                self.editor.message = Some("No path or URL under cursor".to_string());
                return;
            };
            (line_text, byte_offset)
        };

        let bases = open_click_bases(
            self.editor.active_buffer().file_path.as_deref(),
            &self.project_root,
        );
        let Some(resolved) = open_target_at(&line_text, byte_offset, &bases) else {
            self.editor.message = Some("No path or URL under cursor".to_string());
            return;
        };
        match resolved {
            OpenClickTarget::File { path, line } => self.open_clicked_file(&path, line),
            OpenClickTarget::Dir(dir) => self.open_clicked_dir(dir),
            OpenClickTarget::Url(url) => self.open_url_in_browser(&url),
        }
    }

    fn open_clicked_file(&mut self, path: &Path, line: Option<usize>) {
        match line {
            Some(line) => {
                self.open_file_at_char_location(path, line.saturating_sub(1), 0);
            }
            None => {
                // Like open_file_at_char_location, but keeping the cursor
                // where it was for an already-open buffer.
                self.flush_insert_transaction_if_active();
                let jump_before = self.editor.current_jump_location();
                self.editor.open_file(&path.to_string_lossy());
                let jump_after = self.editor.current_jump_location();
                self.record_jump_transition_if_needed(jump_before, jump_after);
                self.emit_plugin_event(PluginEvent::BufferActivated {
                    doc_id: self.editor.active_buffer().id,
                });
                self.queue_active_doc_git_refresh(true);
            }
        }
    }

    fn open_clicked_dir(&mut self, dir: PathBuf) {
        self.queue_git_status_refresh(true);
        let explorer = Explorer::new(dir.clone(), &self.project_root, &self.git_status_cache);
        self.compositor.open_explorer(explorer);
        self.last_used_sidebar = Some(LastUsedSidebar::ExplorerRegular);
        self.editor.message = Some(format!("Explorer: {}", dir.display()));
    }
}

/// Directories relative path candidates resolve against, in order: the
/// clicked buffer's own directory, then the project root.
fn open_click_bases(file_path: Option<&Path>, project_root: &Path) -> Vec<PathBuf> {
    let mut bases: Vec<PathBuf> = Vec::new();
    if let Some(parent) = file_path.and_then(Path::parent)
        && !parent.as_os_str().is_empty()
    {
        bases.push(parent.to_path_buf());
    }
    if !bases.iter().any(|base| base == project_root) {
        bases.push(project_root.to_path_buf());
    }
    bases
}

/// Resolve the token under `offset` in `text`: a web URL span, or a path
/// candidate that exists on disk (relative ones tried against `bases`).
fn open_target_at(text: &str, offset: usize, bases: &[PathBuf]) -> Option<OpenClickTarget> {
    if let Some(span) = crate::ui::url::find_web_url_spans(text)
        .into_iter()
        .find(|span| span.contains_byte(offset))
    {
        return Some(OpenClickTarget::Url(span.as_str(text).to_string()));
    }

    let token = path_token_at(text, offset)?;
    for (candidate, line) in path_candidates(&token) {
        for resolved in resolve_path_candidate(&candidate, bases) {
            if resolved.is_dir() {
                return Some(OpenClickTarget::Dir(resolved));
            }
            if resolved.is_file() {
                return Some(OpenClickTarget::File {
                    path: resolved,
                    line,
                });
            }
        }
    }
    None
}

/// Characters that can be part of a clicked path token. Quotes, brackets
/// and common separators end the token so paths embedded in prose, compiler
/// output or listings come out clean; `:` stays in for `file:line` suffixes.
fn is_path_char(c: char) -> bool {
    if c.is_whitespace() {
        return false;
    }
    !matches!(
        c,
        '"' | '\''
            | '`'
            | '('
            | ')'
            | '['
            | ']'
            | '{'
            | '}'
            | '<'
            | '>'
            | '|'
            | ';'
            | ','
            | '*'
            | '?'
            | '='
            | '（'
            | '）'
            | '「'
            | '」'
            | '。'
            | '、'
            | '：'
    )
}

/// The maximal run of path characters around byte `offset`.
fn path_token_at(text: &str, offset: usize) -> Option<String> {
    if offset >= text.len() {
        return None;
    }
    let mut start = offset;
    while start > 0 {
        let prev = text[..start].chars().next_back()?;
        if !is_path_char(prev) {
            break;
        }
        start -= prev.len_utf8();
    }
    let mut end = offset;
    for c in text[offset..].chars() {
        if !is_path_char(c) {
            break;
        }
        end += c.len_utf8();
    }
    let token = &text[start..end];
    (!token.is_empty()).then(|| token.to_string())
}

/// Interpretations of a token to try against the filesystem, in order:
/// verbatim, with trailing punctuation trimmed, with a `:line[:col]` suffix
/// split off, and each of those without a git-diff `a/`/`b/` prefix.
fn path_candidates(token: &str) -> Vec<(String, Option<usize>)> {
    fn push(out: &mut Vec<(String, Option<usize>)>, candidate: &str, line: Option<usize>) {
        if !candidate.is_empty()
            && candidate != "."
            && !out.iter().any(|(existing, _)| existing == candidate)
        {
            out.push((candidate.to_string(), line));
        }
    }

    let mut out: Vec<(String, Option<usize>)> = Vec::new();
    push(&mut out, token, None);
    let trimmed = token.trim_end_matches(['.', ',', ';', ':', '!']);
    push(&mut out, trimmed, None);
    if let Some((base, line)) = split_line_suffix(trimmed) {
        push(&mut out, base, Some(line));
    }
    for index in 0..out.len() {
        let (candidate, line) = out[index].clone();
        if let Some(rest) = candidate
            .strip_prefix("a/")
            .or_else(|| candidate.strip_prefix("b/"))
        {
            push(&mut out, rest, line);
        }
    }
    out
}

/// Split a trailing `:line` or `:line:col` off a token, returning the base
/// and the line number.
fn split_line_suffix(token: &str) -> Option<(&str, usize)> {
    let mut base = token;
    let mut line = None;
    for _ in 0..2 {
        let Some(index) = base.rfind(':') else { break };
        let digits = &base[index + 1..];
        if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
            break;
        }
        line = digits.parse::<usize>().ok();
        base = &base[..index];
    }
    let line = line?;
    (!base.is_empty()).then_some((base, line))
}

/// Absolute and `~` candidates resolve directly; relative ones are tried
/// against each base in order.
fn resolve_path_candidate(candidate: &str, bases: &[PathBuf]) -> Vec<PathBuf> {
    if candidate == "~" {
        return std::env::var_os("HOME")
            .map(PathBuf::from)
            .into_iter()
            .collect();
    }
    if let Some(rest) = candidate.strip_prefix("~/") {
        return std::env::var_os("HOME")
            .map(|home| Path::new(&home).join(rest))
            .into_iter()
            .collect();
    }
    let expanded = PathBuf::from(candidate);
    if expanded.is_absolute() {
        return vec![expanded];
    }
    bases.iter().map(|base| base.join(candidate)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_extraction_stops_at_delimiters() {
        let text = "error in (src/main.rs:42:7): expected";
        let offset = text.find("main").unwrap();
        assert_eq!(
            path_token_at(text, offset).as_deref(),
            Some("src/main.rs:42:7")
        );
        assert_eq!(path_token_at("  ", 1), None);
    }

    #[test]
    fn line_suffix_split() {
        assert_eq!(split_line_suffix("a.rs:42"), Some(("a.rs", 42)));
        assert_eq!(split_line_suffix("a.rs:42:7"), Some(("a.rs", 42)));
        assert_eq!(split_line_suffix("a.rs"), None);
        assert_eq!(split_line_suffix(":42"), None);
    }

    #[test]
    fn candidates_cover_punctuation_line_and_diff_prefixes() {
        let candidates = path_candidates("b/src/lib.rs:10,");
        assert!(
            candidates
                .iter()
                .any(|(name, line)| name == "src/lib.rs" && *line == Some(10))
        );
    }

    #[test]
    fn relative_candidates_try_each_base_in_order() {
        let bases = vec![PathBuf::from("/doc/dir"), PathBuf::from("/root")];
        assert_eq!(
            resolve_path_candidate("x.rs", &bases),
            vec![PathBuf::from("/doc/dir/x.rs"), PathBuf::from("/root/x.rs")]
        );
        assert_eq!(
            resolve_path_candidate("/abs", &bases),
            vec![PathBuf::from("/abs")]
        );
    }

    #[test]
    fn open_target_at_finds_files_dirs_and_urls() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir(dir.path().join("sub")).expect("mkdir");
        std::fs::write(dir.path().join("sub/file.rs"), "x").expect("write");
        let bases = vec![dir.path().to_path_buf()];

        let text = "see sub/file.rs:3 and sub or https://example.com now";
        assert_eq!(
            open_target_at(text, text.find("file.rs").unwrap(), &bases),
            Some(OpenClickTarget::File {
                path: dir.path().join("sub/file.rs"),
                line: Some(3),
            })
        );
        assert_eq!(
            open_target_at(text, text.find("sub ").unwrap(), &bases),
            Some(OpenClickTarget::Dir(dir.path().join("sub")))
        );
        assert_eq!(
            open_target_at(text, text.find("example").unwrap(), &bases),
            Some(OpenClickTarget::Url("https://example.com".to_string()))
        );
        assert_eq!(
            open_target_at(text, text.find("now").unwrap(), &bases),
            None
        );
    }
}
