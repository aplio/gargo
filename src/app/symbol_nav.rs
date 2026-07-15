//! Fuzzy goto-definition backed by the workspace tree-sitter symbol index.
//!
//! This is the no-LSP fallback: an exact-name lookup over ctags-style tag
//! definitions, so hits can be false positives. Candidates are ranked (same
//! file > same directory > rest) and ambiguity is surfaced through the
//! reference picker instead of pretending there is a single true answer.

use super::*;

use std::sync::Arc;

use crate::command::symbol_index::{SymbolIndex, SymbolLocation, rank_symbol_hits};
use crate::core::document::DocumentId;
use crate::core::document::expand::word_range_at_caret;
use crate::syntax::symbol::extract_definition_sections;

/// Buffers longer than this skip the live-rope overlay (mirrors the index's
/// own file-size cap).
const MAX_OVERLAY_CHARS: usize = 2 * 1024 * 1024;
const MAX_PICKER_ENTRIES: usize = 100;

impl App {
    pub(super) fn goto_definition_via_symbol_index(&mut self) {
        let Some(name) = self.identifier_under_cursor() else {
            self.editor.message = Some("No identifier under cursor".to_string());
            return;
        };
        let index = self.ensure_symbol_index();
        if !index.is_ready() {
            self.editor.message =
                Some("Symbol index is building — try again in a moment".to_string());
            return;
        }

        let mut hits = index.lookup(&name);
        let current_rel = self.active_buffer_project_rel_path();
        self.overlay_active_buffer_definitions(&name, current_rel.as_deref(), &mut hits);
        rank_symbol_hits(&mut hits, current_rel.as_deref());

        match hits.len() {
            0 => {
                self.editor.message = Some(format!("No definition found for '{name}'"));
            }
            1 => {
                let hit = &hits[0];
                let path = self.project_root.join(&hit.rel_path);
                self.open_file_at_char_location(&path, hit.line, hit.char_col);
            }
            _ => self.open_symbol_definitions_picker(&name, hits),
        }
    }

    pub(super) fn ensure_symbol_index(&mut self) -> Arc<SymbolIndex> {
        if let Some(index) = &self.symbol_index {
            return Arc::clone(index);
        }
        let index = SymbolIndex::new(self.project_root.clone());
        index.request_refresh();
        self.symbol_index = Some(Arc::clone(&index));
        index
    }

    /// Re-index a just-saved document. No-op while the index has never been
    /// built (lazy start) or when the file lives outside the project root.
    pub(super) fn update_symbol_index_for_saved_doc(&mut self, doc_id: DocumentId) {
        let Some(index) = &self.symbol_index else {
            return;
        };
        let Some(doc) = self.editor.buffers().iter().find(|d| d.id == doc_id) else {
            return;
        };
        let Some(path) = doc.file_path.as_deref() else {
            return;
        };
        if let Some(rel_path) = self.project_rel_path(path) {
            index.update_file(rel_path);
        }
    }

    fn identifier_under_cursor(&self) -> Option<String> {
        let doc = self.editor.active_buffer();
        let pos = doc.cursors.first().copied()?;
        let (start, end) = word_range_at_caret(&doc.rope, pos)?;
        let word: String = doc.rope.slice(start..end).to_string();
        let is_identifier = word.chars().all(|c| c.is_alphanumeric() || c == '_')
            && word.chars().any(|c| !c.is_ascii_digit());
        is_identifier.then_some(word)
    }

    /// Replace the active buffer's index hits with definitions extracted from
    /// its live rope, so same-file jumps track unsaved edits instead of the
    /// on-disk state the background index saw.
    fn overlay_active_buffer_definitions(
        &self,
        name: &str,
        current_rel: Option<&str>,
        hits: &mut Vec<SymbolLocation>,
    ) {
        let Some(current_rel) = current_rel else {
            return;
        };
        let doc = self.editor.active_buffer();
        let Some(path) = doc.file_path.as_deref() else {
            return;
        };
        let Some(lang_def) = self
            .editor
            .language_registry
            .detect_by_extension(&path.to_string_lossy())
        else {
            return;
        };
        if lang_def.tags_query.is_none()
            || lang_def.name == "Markdown"
            || doc.rope.len_chars() > MAX_OVERLAY_CHARS
        {
            return;
        }

        let text = doc.rope.to_string();
        hits.retain(|hit| hit.rel_path != current_rel);
        for section in extract_definition_sections(&text, lang_def) {
            if section.name == name {
                hits.push(SymbolLocation {
                    rel_path: current_rel.to_string(),
                    line: section.line,
                    char_col: section.char_col,
                    kind: section.kind,
                });
            }
        }
    }

    fn open_symbol_definitions_picker(&mut self, name: &str, hits: Vec<SymbolLocation>) {
        let entries: Vec<ReferencePickerEntry> = hits
            .into_iter()
            .take(MAX_PICKER_ENTRIES)
            .map(|hit| {
                let path = self.project_root.join(&hit.rel_path);
                let (preview_lines, target_preview_line, target_char_col) =
                    self.reference_preview_lines_for_char_location(&path, hit.line, hit.char_col);
                let target_line_text = target_preview_line
                    .and_then(|line_idx| preview_lines.get(line_idx))
                    .and_then(|line| Self::jump_line_text_from_preview_line(line));
                let character_utf16 = target_line_text
                    .map(|text| Self::utf16_col_from_char(text, hit.char_col))
                    .unwrap_or(hit.char_col);
                let label = format!(
                    "{} [{}]",
                    self.reference_label_for_location(
                        &path,
                        hit.line,
                        target_char_col,
                        target_line_text
                    ),
                    hit.kind
                );

                ReferencePickerEntry {
                    label,
                    path: path.clone(),
                    line: hit.line,
                    character_utf16,
                    preview_lines,
                    source_path: Some(path.to_string_lossy().to_string()),
                    target_preview_line,
                    target_char_col,
                }
            })
            .collect();

        let palette = Palette::new_reference_picker(format!("Definitions of '{name}'"), entries);
        self.compositor.push_palette(palette);
    }

    fn active_buffer_project_rel_path(&self) -> Option<String> {
        let path = self.editor.active_buffer().file_path.clone()?;
        self.project_rel_path(&path)
    }

    /// Project-root-relative, '/'-separated path (the symbol index's key
    /// convention). Relative buffer paths are assumed root-relative.
    fn project_rel_path(&self, path: &Path) -> Option<String> {
        let rel = if path.is_absolute() {
            path.strip_prefix(&self.project_root).ok()?
        } else {
            path
        };
        let rel = rel.to_string_lossy();
        if std::path::MAIN_SEPARATOR == '/' {
            Some(rel.into_owned())
        } else {
            Some(rel.replace(std::path::MAIN_SEPARATOR, "/"))
        }
    }
}
