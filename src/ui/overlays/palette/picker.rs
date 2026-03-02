use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use unicode_width::UnicodeWidthChar;

use crossterm::style::Color;

use crate::command::git::GitFileStatus;
use crate::command::history::CommandHistory;
use crate::command::registry::CommandRegistry;
use crate::config::Config;
use crate::core::buffer::BufferId;
use crate::input::action::{
    Action, AppAction, BufferAction, IntegrationAction, NavigationAction, ProjectAction, UiAction,
};
use crate::log::debug_log;
use crate::syntax::highlight::{HighlightSpan, highlight_text};
use crate::syntax::language::LanguageRegistry;
use crate::syntax::theme::Theme;
use crate::ui::framework::cell::CellStyle;
use crate::ui::framework::component::EventResult;
use crate::ui::framework::surface::Surface;
use crate::ui::shared::filtering::{fuzzy_match, fzf_style_match};
use crate::ui::views::text_view::render_highlighted_line;
use crate::core_lib::text::input::TextInput;
use crate::core_lib::ui::text::{display_width, truncate_to_width};

#[path = "workers.rs"]
mod workers;

type PreviewCache = HashMap<String, (Vec<String>, HashMap<usize, Vec<HighlightSpan>>)>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteMode {
    Command,
    FileFinder,
    BufferPicker,
    JumpPicker,
    ReferencePicker,
    GitBranchPicker,
    SymbolPicker,
    GlobalSearch,
    GotoLine,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CandidateKind {
    Command(usize),
    Buffer(BufferId),
    Jump(usize),
    Reference(usize),
    GitBranch(usize),
    Symbol(usize),
    File(usize),
    SearchResult(usize),
}

pub struct ScoredCandidate {
    pub kind: CandidateKind,
    pub label: String,
    pub score: i32,
    pub match_positions: Vec<usize>,
    pub preview_lines: Vec<String>,
}

struct PreviewRequest {
    rel_path: String,
}

struct PreviewResult {
    rel_path: String,
    lines: Vec<String>,
    spans: HashMap<usize, Vec<HighlightSpan>>,
}

#[derive(Debug, Clone)]
struct GlobalSearchResultEntry {
    rel_path: String,
    line: usize,
    char_col: usize,
    preview_lines: Vec<String>,
}

#[derive(Debug, Clone)]
struct JumpEntry {
    jump_index: usize,
    label: String,
    preview_lines: Vec<String>,
    source_path: Option<String>,
    target_preview_line: Option<usize>,
    target_char_col: usize,
}

#[derive(Debug, Clone)]
pub struct JumpPickerEntry {
    pub jump_index: usize,
    pub label: String,
    pub preview_lines: Vec<String>,
    pub source_path: Option<String>,
    pub target_preview_line: Option<usize>,
    pub target_char_col: usize,
}

#[derive(Debug, Clone)]
struct ReferenceEntry {
    label: String,
    path: PathBuf,
    line: usize,
    character_utf16: usize,
    preview_lines: Vec<String>,
    source_path: Option<String>,
    target_preview_line: Option<usize>,
    target_char_col: usize,
}

#[derive(Debug, Clone)]
pub struct ReferencePickerEntry {
    pub label: String,
    pub path: PathBuf,
    pub line: usize,
    pub character_utf16: usize,
    pub preview_lines: Vec<String>,
    pub source_path: Option<String>,
    pub target_preview_line: Option<usize>,
    pub target_char_col: usize,
}

#[derive(Debug, Clone)]
struct SymbolEntry {
    label: String,
    line: usize,
    char_col: usize,
    preview_lines: Vec<String>,
    copy_text: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SmartCopyPickerEntry {
    pub label: String,
    pub line: usize,
    pub char_col: usize,
    pub preview_lines: Vec<String>,
    pub copy_text: String,
}

#[derive(Debug, Clone)]
struct GitBranchEntry {
    branch_name: String,
    label: String,
    preview_lines: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct GitBranchPickerEntry {
    pub branch_name: String,
    pub label: String,
    pub preview_lines: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SymbolSubmitBehavior {
    JumpToLocation,
    CopyToClipboard,
}

struct GlobalSearchRequest {
    query: String,
    generation: u64,
}

struct GlobalSearchBatch {
    generation: u64,
    results: Vec<GlobalSearchResultEntry>,
    error: Option<String>,
}

fn split_numbered_preview_line(line: &str) -> Option<(&str, &str)> {
    let (prefix, right) = line.split_once('|')?;
    let code = right.strip_prefix(' ').unwrap_or(right);
    Some((prefix, code))
}

fn jump_marker_column(line: &str, target_char_col: usize) -> Option<(usize, usize)> {
    let (prefix, code) = split_numbered_preview_line(line)?;
    let prefix_display_width = display_width(prefix) + 2; // "| "
    let chars: Vec<char> = code.chars().collect();
    if chars.is_empty() {
        return Some((prefix_display_width, 1));
    }
    let clamped = target_char_col.min(chars.len().saturating_sub(1));
    let char_byte = code
        .char_indices()
        .nth(clamped)
        .map(|(idx, _)| idx)
        .unwrap_or(code.len());
    let code_display_width = display_width(&code[..char_byte]);
    let ch_width = UnicodeWidthChar::width(chars[clamped]).unwrap_or(1).max(1);
    Some((prefix_display_width + code_display_width, ch_width))
}

struct PreviewHorizontalWindow<'a> {
    visible: &'a str,
    start_byte: usize,
    end_byte: usize,
    start_col: usize,
    used_width: usize,
}

fn slice_preview_display_window(
    display: &str,
    start_col: usize,
    max_width: usize,
) -> PreviewHorizontalWindow<'_> {
    let mut col = 0usize;
    let mut start_byte = display.len();
    let mut effective_start_col = 0usize;

    for (i, ch) in display.char_indices() {
        let ch_w = UnicodeWidthChar::width(ch).unwrap_or(0);
        if col >= start_col {
            start_byte = i;
            effective_start_col = col;
            break;
        }
        if col + ch_w > start_col {
            // Never render half of a wide character.
            col += ch_w;
            continue;
        }
        col += ch_w;
    }

    if start_byte == display.len() {
        effective_start_col = col;
    }

    if max_width == 0 || start_byte == display.len() {
        return PreviewHorizontalWindow {
            visible: "",
            start_byte,
            end_byte: start_byte,
            start_col: effective_start_col,
            used_width: 0,
        };
    }

    let mut used_width = 0usize;
    let mut end_byte = display.len();
    for (i, ch) in display[start_byte..].char_indices() {
        let ch_w = UnicodeWidthChar::width(ch).unwrap_or(0);
        if used_width + ch_w > max_width {
            end_byte = start_byte + i;
            break;
        }
        used_width += ch_w;
    }

    PreviewHorizontalWindow {
        visible: &display[start_byte..end_byte],
        start_byte,
        end_byte,
        start_col: effective_start_col,
        used_width,
    }
}

fn rebase_preview_spans_to_window(
    spans: &[HighlightSpan],
    start_byte: usize,
    end_byte: usize,
) -> Vec<HighlightSpan> {
    spans
        .iter()
        .filter_map(|span| {
            let overlap_start = span.start.max(start_byte);
            let overlap_end = span.end.min(end_byte);
            if overlap_start >= overlap_end {
                return None;
            }
            Some(HighlightSpan {
                start: overlap_start - start_byte,
                end: overlap_end - start_byte,
                capture_name: span.capture_name.clone(),
            })
        })
        .collect()
}

fn command_display_label(
    entry: &crate::command::registry::CommandEntry,
    config: &Config,
) -> String {
    match entry.id.as_str() {
        "config.toggle_debug" => {
            if config.debug {
                "Hide Debug".to_string()
            } else {
                "Show Debug".to_string()
            }
        }
        "config.toggle_line_numbers" => {
            if config.show_line_number {
                "Hide Line Number".to_string()
            } else {
                "Show Line Number".to_string()
            }
        }
        _ => entry.label.clone(),
    }
}

fn command_preview_lines(
    entry: &crate::command::registry::CommandEntry,
    display_label: &str,
) -> Vec<String> {
    let mut lines = vec![format!("Command: {}", display_label)];
    if let Some(category) = &entry.category {
        lines.push(format!("Category: {}", category));
    }
    lines.push(format!("ID: {}", entry.id));

    if entry.id == "core.copy_gargo_version" {
        lines.push(String::new());
        lines.push("Version Preview:".to_string());
        lines.push(crate::command::registry::gargo_version_info());
        lines.push(
            "Gargo is a Rust terminal text editor with modal editing, multi-buffer, and Tree-sitter highlighting."
                .to_string(),
        );
        lines.push("Executes: copy version info to system clipboard.".to_string());
    }

    lines
}

pub struct Palette {
    pub input: TextInput,
    pub mode: PaletteMode,
    pub candidates: Vec<ScoredCandidate>,
    pub selected: usize,
    pub scroll_offset: usize,
    pub preview_lines: Vec<String>,
    pub preview_spans: HashMap<usize, Vec<HighlightSpan>>,
    preview_cache: PreviewCache,
    buffer_entries: Vec<(BufferId, String, Vec<String>)>,
    jump_entries: Vec<JumpEntry>,
    reference_entries: Vec<ReferenceEntry>,
    git_branch_entries: Vec<GitBranchEntry>,
    symbol_entries: Vec<SymbolEntry>,
    symbol_submit_behavior: SymbolSubmitBehavior,
    file_entries: Vec<String>,
    project_root: PathBuf,
    request_tx: Option<mpsc::Sender<PreviewRequest>>,
    result_rx: Option<mpsc::Receiver<PreviewResult>>,
    _worker: Option<thread::JoinHandle<()>>,
    requested_paths: HashSet<String>,
    git_status_map: HashMap<String, GitFileStatus>,
    last_previewed_buffer: Option<BufferId>,
    last_previewed_jump_index: Option<usize>,
    last_previewed_reference_index: Option<usize>,
    last_previewed_git_branch_index: Option<usize>,
    last_previewed_symbol_index: Option<usize>,
    jump_target_preview_line: Option<usize>,
    jump_target_char_col: Option<usize>,
    buffer_highlight_cache: HashMap<BufferId, HashMap<usize, Vec<HighlightSpan>>>,
    reference_highlight_cache: HashMap<usize, HashMap<usize, Vec<HighlightSpan>>>,
    lang_registry_owned: Option<LanguageRegistry>,
    command_history: Option<Rc<CommandHistory>>,
    global_search_entries: Vec<GlobalSearchResultEntry>,
    global_search_request_tx: Option<mpsc::Sender<GlobalSearchRequest>>,
    global_search_result_rx: Option<mpsc::Receiver<GlobalSearchBatch>>,
    _global_search_worker: Option<thread::JoinHandle<()>>,
    global_search_generation: u64,
    global_search_latest_applied: u64,
    global_search_dirty: bool,
    global_search_changed_at: Option<Instant>,
    active_doc_lines: Vec<String>,
    is_unified: bool,
    caller_label: Option<String>,
}

impl Palette {
    pub fn new(
        files: Vec<String>,
        project_root: &Path,
        git_status_map: &HashMap<String, GitFileStatus>,
        command_history: Option<Rc<CommandHistory>>,
        symbols: Vec<(String, usize, usize, Vec<String>)>,
        active_doc_lines: Vec<String>,
    ) -> Self {
        let (req_tx, req_rx) = mpsc::channel::<PreviewRequest>();
        let (res_tx, res_rx) = mpsc::channel::<PreviewResult>();
        let root = project_root.to_path_buf();
        let handle = thread::spawn(move || {
            workers::preview_worker(req_rx, res_tx, root);
        });

        let symbol_entries: Vec<SymbolEntry> = symbols
            .into_iter()
            .map(|(label, line, char_col, preview_lines)| SymbolEntry {
                label,
                line,
                char_col,
                preview_lines,
                copy_text: None,
            })
            .collect();

        Self {
            input: TextInput::new(">".into(), 1),
            mode: PaletteMode::Command,
            candidates: Vec::new(),
            selected: 0,
            scroll_offset: 0,
            preview_lines: Vec::new(),
            preview_spans: HashMap::new(),
            preview_cache: HashMap::new(),
            buffer_entries: Vec::new(),
            jump_entries: Vec::new(),
            reference_entries: Vec::new(),
            git_branch_entries: Vec::new(),
            symbol_entries,
            symbol_submit_behavior: SymbolSubmitBehavior::JumpToLocation,
            file_entries: files,
            project_root: project_root.to_path_buf(),
            request_tx: Some(req_tx),
            result_rx: Some(res_rx),
            _worker: Some(handle),
            requested_paths: HashSet::new(),
            git_status_map: git_status_map.clone(),
            last_previewed_buffer: None,
            last_previewed_jump_index: None,
            last_previewed_reference_index: None,
            last_previewed_git_branch_index: None,
            last_previewed_symbol_index: None,
            jump_target_preview_line: None,
            jump_target_char_col: None,
            buffer_highlight_cache: HashMap::new(),
            reference_highlight_cache: HashMap::new(),
            lang_registry_owned: None,
            command_history,
            global_search_entries: Vec::new(),
            global_search_request_tx: None,
            global_search_result_rx: None,
            _global_search_worker: None,
            global_search_generation: 0,
            global_search_latest_applied: 0,
            global_search_dirty: false,
            global_search_changed_at: None,
            active_doc_lines,
            is_unified: true,
            caller_label: None,
        }
    }

    pub fn new_buffer_picker(entries: Vec<(BufferId, String, Vec<String>)>) -> Self {
        let candidates = entries
            .iter()
            .map(|(id, name, _)| ScoredCandidate {
                kind: CandidateKind::Buffer(*id),
                label: name.clone(),
                score: 0,
                match_positions: Vec::new(),
                preview_lines: Vec::new(),
            })
            .collect();
        let mut palette = Self {
            input: TextInput::default(),
            mode: PaletteMode::BufferPicker,
            candidates,
            selected: 0,
            scroll_offset: 0,
            preview_lines: Vec::new(),
            preview_spans: HashMap::new(),
            preview_cache: HashMap::new(),
            buffer_entries: entries,
            jump_entries: Vec::new(),
            reference_entries: Vec::new(),
            git_branch_entries: Vec::new(),
            symbol_entries: Vec::new(),
            symbol_submit_behavior: SymbolSubmitBehavior::JumpToLocation,
            file_entries: Vec::new(),
            project_root: PathBuf::new(),
            request_tx: None,
            result_rx: None,
            _worker: None,
            requested_paths: HashSet::new(),
            git_status_map: HashMap::new(),
            last_previewed_buffer: None,
            last_previewed_jump_index: None,
            last_previewed_reference_index: None,
            last_previewed_git_branch_index: None,
            last_previewed_symbol_index: None,
            jump_target_preview_line: None,
            jump_target_char_col: None,
            buffer_highlight_cache: HashMap::new(),
            reference_highlight_cache: HashMap::new(),
            lang_registry_owned: Some(LanguageRegistry::new()),
            command_history: None,
            global_search_entries: Vec::new(),
            global_search_request_tx: None,
            global_search_result_rx: None,
            _global_search_worker: None,
            global_search_generation: 0,
            global_search_latest_applied: 0,
            global_search_dirty: false,
            global_search_changed_at: None,
            active_doc_lines: Vec::new(),
            is_unified: false,
            caller_label: None,
        };
        palette.update_buffer_preview();
        palette
    }

    pub fn set_git_status_map(&mut self, git_status_map: &HashMap<String, GitFileStatus>) {
        self.git_status_map = git_status_map.clone();
    }

    fn restart_global_search_worker(&mut self) {
        if self.global_search_request_tx.is_none() {
            return;
        }

        self.global_search_request_tx = None;
        self.global_search_result_rx = None;
        self._global_search_worker = None;

        let (search_req_tx, search_req_rx) = mpsc::channel::<GlobalSearchRequest>();
        let (search_res_tx, search_res_rx) = mpsc::channel::<GlobalSearchBatch>();
        let root = self.project_root.clone();
        let worker_files = self.file_entries.clone();
        let search_handle = thread::spawn(move || {
            workers::global_search_worker(search_req_rx, search_res_tx, root, worker_files);
        });

        self.global_search_request_tx = Some(search_req_tx);
        self.global_search_result_rx = Some(search_res_rx);
        self._global_search_worker = Some(search_handle);
        self.global_search_entries.clear();
        self.candidates.clear();
        self.selected = 0;
        self.preview_lines.clear();
        self.preview_spans.clear();
        self.global_search_generation = 0;
        self.global_search_latest_applied = 0;
        self.global_search_dirty = true;
        self.global_search_changed_at = Some(Instant::now());
    }

    pub fn set_file_entries(&mut self, files: Vec<String>) {
        self.file_entries = files;
        self.requested_paths.clear();
        self.preview_cache.clear();
        self.restart_global_search_worker();
    }

    pub fn set_git_branch_entries(&mut self, entries: Vec<GitBranchPickerEntry>) {
        self.git_branch_entries = entries
            .into_iter()
            .map(|entry| GitBranchEntry {
                branch_name: entry.branch_name,
                label: entry.label,
                preview_lines: entry.preview_lines,
            })
            .collect();
        self.last_previewed_git_branch_index = None;

        if self.mode == PaletteMode::GitBranchPicker {
            self.filter_git_branch_candidates();
            self.update_git_branch_preview();
        }
    }

    pub fn refresh_after_file_entries_update(
        &mut self,
        registry: &CommandRegistry,
        lang_registry: &LanguageRegistry,
        config: &Config,
    ) {
        if self.is_unified {
            self.update_candidates(registry, lang_registry, config);
            return;
        }

        if self.mode == PaletteMode::GlobalSearch {
            self.mark_global_search_dirty();
            self.pump_global_search();
        }
    }

    pub fn new_jump_picker(entries: Vec<JumpPickerEntry>) -> Self {
        let jump_entries: Vec<JumpEntry> = entries
            .into_iter()
            .map(|entry| JumpEntry {
                jump_index: entry.jump_index,
                label: entry.label,
                preview_lines: entry.preview_lines,
                source_path: entry.source_path,
                target_preview_line: entry.target_preview_line,
                target_char_col: entry.target_char_col,
            })
            .collect();
        let candidates = jump_entries
            .iter()
            .map(|entry| ScoredCandidate {
                kind: CandidateKind::Jump(entry.jump_index),
                label: entry.label.clone(),
                score: 0,
                match_positions: Vec::new(),
                preview_lines: Vec::new(),
            })
            .collect();
        let mut palette = Self {
            input: TextInput::default(),
            mode: PaletteMode::JumpPicker,
            candidates,
            selected: 0,
            scroll_offset: 0,
            preview_lines: Vec::new(),
            preview_spans: HashMap::new(),
            preview_cache: HashMap::new(),
            buffer_entries: Vec::new(),
            jump_entries,
            reference_entries: Vec::new(),
            git_branch_entries: Vec::new(),
            symbol_entries: Vec::new(),
            symbol_submit_behavior: SymbolSubmitBehavior::JumpToLocation,
            file_entries: Vec::new(),
            project_root: PathBuf::new(),
            request_tx: None,
            result_rx: None,
            _worker: None,
            requested_paths: HashSet::new(),
            git_status_map: HashMap::new(),
            last_previewed_buffer: None,
            last_previewed_jump_index: None,
            last_previewed_reference_index: None,
            last_previewed_git_branch_index: None,
            last_previewed_symbol_index: None,
            jump_target_preview_line: None,
            jump_target_char_col: None,
            buffer_highlight_cache: HashMap::new(),
            reference_highlight_cache: HashMap::new(),
            lang_registry_owned: None,
            command_history: None,
            global_search_entries: Vec::new(),
            global_search_request_tx: None,
            global_search_result_rx: None,
            _global_search_worker: None,
            global_search_generation: 0,
            global_search_latest_applied: 0,
            global_search_dirty: false,
            global_search_changed_at: None,
            active_doc_lines: Vec::new(),
            is_unified: false,
            caller_label: None,
        };
        palette.update_jump_preview();
        palette
    }

    pub fn new_reference_picker(caller_label: String, entries: Vec<ReferencePickerEntry>) -> Self {
        let reference_entries: Vec<ReferenceEntry> = entries
            .into_iter()
            .map(|entry| ReferenceEntry {
                label: entry.label,
                path: entry.path,
                line: entry.line,
                character_utf16: entry.character_utf16,
                preview_lines: entry.preview_lines,
                source_path: entry.source_path,
                target_preview_line: entry.target_preview_line,
                target_char_col: entry.target_char_col,
            })
            .collect();
        let candidates = reference_entries
            .iter()
            .enumerate()
            .map(|(idx, entry)| ScoredCandidate {
                kind: CandidateKind::Reference(idx),
                label: entry.label.clone(),
                score: 0,
                match_positions: Vec::new(),
                preview_lines: Vec::new(),
            })
            .collect();
        let mut palette = Self {
            input: TextInput::default(),
            mode: PaletteMode::ReferencePicker,
            candidates,
            selected: 0,
            scroll_offset: 0,
            preview_lines: Vec::new(),
            preview_spans: HashMap::new(),
            preview_cache: HashMap::new(),
            buffer_entries: Vec::new(),
            jump_entries: Vec::new(),
            reference_entries,
            git_branch_entries: Vec::new(),
            symbol_entries: Vec::new(),
            symbol_submit_behavior: SymbolSubmitBehavior::JumpToLocation,
            file_entries: Vec::new(),
            project_root: PathBuf::new(),
            request_tx: None,
            result_rx: None,
            _worker: None,
            requested_paths: HashSet::new(),
            git_status_map: HashMap::new(),
            last_previewed_buffer: None,
            last_previewed_jump_index: None,
            last_previewed_reference_index: None,
            last_previewed_git_branch_index: None,
            last_previewed_symbol_index: None,
            jump_target_preview_line: None,
            jump_target_char_col: None,
            buffer_highlight_cache: HashMap::new(),
            reference_highlight_cache: HashMap::new(),
            lang_registry_owned: None,
            command_history: None,
            global_search_entries: Vec::new(),
            global_search_request_tx: None,
            global_search_result_rx: None,
            _global_search_worker: None,
            global_search_generation: 0,
            global_search_latest_applied: 0,
            global_search_dirty: false,
            global_search_changed_at: None,
            active_doc_lines: Vec::new(),
            is_unified: false,
            caller_label: Some(caller_label),
        };
        palette.update_reference_preview();
        palette
    }

    pub fn new_git_branch_picker(entries: Vec<GitBranchPickerEntry>) -> Self {
        let git_branch_entries: Vec<GitBranchEntry> = entries
            .into_iter()
            .map(|entry| GitBranchEntry {
                branch_name: entry.branch_name,
                label: entry.label,
                preview_lines: entry.preview_lines,
            })
            .collect();
        let selected = git_branch_entries
            .iter()
            .position(|entry| entry.label.starts_with("* "))
            .unwrap_or(0);
        let candidates = git_branch_entries
            .iter()
            .enumerate()
            .map(|(idx, entry)| ScoredCandidate {
                kind: CandidateKind::GitBranch(idx),
                label: entry.label.clone(),
                score: 0,
                match_positions: Vec::new(),
                preview_lines: Vec::new(),
            })
            .collect();
        let mut palette = Self {
            input: TextInput::default(),
            mode: PaletteMode::GitBranchPicker,
            candidates,
            selected,
            scroll_offset: 0,
            preview_lines: Vec::new(),
            preview_spans: HashMap::new(),
            preview_cache: HashMap::new(),
            buffer_entries: Vec::new(),
            jump_entries: Vec::new(),
            reference_entries: Vec::new(),
            git_branch_entries,
            symbol_entries: Vec::new(),
            symbol_submit_behavior: SymbolSubmitBehavior::JumpToLocation,
            file_entries: Vec::new(),
            project_root: PathBuf::new(),
            request_tx: None,
            result_rx: None,
            _worker: None,
            requested_paths: HashSet::new(),
            git_status_map: HashMap::new(),
            last_previewed_buffer: None,
            last_previewed_jump_index: None,
            last_previewed_reference_index: None,
            last_previewed_git_branch_index: None,
            last_previewed_symbol_index: None,
            jump_target_preview_line: None,
            jump_target_char_col: None,
            buffer_highlight_cache: HashMap::new(),
            reference_highlight_cache: HashMap::new(),
            lang_registry_owned: None,
            command_history: None,
            global_search_entries: Vec::new(),
            global_search_request_tx: None,
            global_search_result_rx: None,
            _global_search_worker: None,
            global_search_generation: 0,
            global_search_latest_applied: 0,
            global_search_dirty: false,
            global_search_changed_at: None,
            active_doc_lines: Vec::new(),
            is_unified: false,
            caller_label: Some("Git Branches".to_string()),
        };
        palette.update_git_branch_preview();
        palette
    }

    pub fn new_symbol_picker(entries: Vec<(String, usize, usize, Vec<String>)>) -> Self {
        let symbol_entries: Vec<SymbolEntry> = entries
            .into_iter()
            .map(|(label, line, char_col, preview_lines)| SymbolEntry {
                label,
                line,
                char_col,
                preview_lines,
                copy_text: None,
            })
            .collect();
        let candidates = symbol_entries
            .iter()
            .enumerate()
            .map(|(idx, entry)| ScoredCandidate {
                kind: CandidateKind::Symbol(idx),
                label: entry.label.clone(),
                score: 0,
                match_positions: Vec::new(),
                preview_lines: Vec::new(),
            })
            .collect();
        let mut palette = Self {
            input: TextInput::default(),
            mode: PaletteMode::SymbolPicker,
            candidates,
            selected: 0,
            scroll_offset: 0,
            preview_lines: Vec::new(),
            preview_spans: HashMap::new(),
            preview_cache: HashMap::new(),
            buffer_entries: Vec::new(),
            jump_entries: Vec::new(),
            reference_entries: Vec::new(),
            git_branch_entries: Vec::new(),
            symbol_entries,
            symbol_submit_behavior: SymbolSubmitBehavior::JumpToLocation,
            file_entries: Vec::new(),
            project_root: PathBuf::new(),
            request_tx: None,
            result_rx: None,
            _worker: None,
            requested_paths: HashSet::new(),
            git_status_map: HashMap::new(),
            last_previewed_buffer: None,
            last_previewed_jump_index: None,
            last_previewed_reference_index: None,
            last_previewed_git_branch_index: None,
            last_previewed_symbol_index: None,
            jump_target_preview_line: None,
            jump_target_char_col: None,
            buffer_highlight_cache: HashMap::new(),
            reference_highlight_cache: HashMap::new(),
            lang_registry_owned: None,
            command_history: None,
            global_search_entries: Vec::new(),
            global_search_request_tx: None,
            global_search_result_rx: None,
            _global_search_worker: None,
            global_search_generation: 0,
            global_search_latest_applied: 0,
            global_search_dirty: false,
            global_search_changed_at: None,
            active_doc_lines: Vec::new(),
            is_unified: false,
            caller_label: None,
        };
        palette.update_symbol_preview();
        palette
    }

    pub fn new_smart_copy_picker(entries: Vec<SmartCopyPickerEntry>) -> Self {
        let symbol_entries: Vec<SymbolEntry> = entries
            .into_iter()
            .map(|entry| SymbolEntry {
                label: entry.label,
                line: entry.line,
                char_col: entry.char_col,
                preview_lines: entry.preview_lines,
                copy_text: Some(entry.copy_text),
            })
            .collect();
        let candidates = symbol_entries
            .iter()
            .enumerate()
            .map(|(idx, entry)| ScoredCandidate {
                kind: CandidateKind::Symbol(idx),
                label: entry.label.clone(),
                score: 0,
                match_positions: Vec::new(),
                preview_lines: Vec::new(),
            })
            .collect();
        let mut palette = Self {
            input: TextInput::default(),
            mode: PaletteMode::SymbolPicker,
            candidates,
            selected: 0,
            scroll_offset: 0,
            preview_lines: Vec::new(),
            preview_spans: HashMap::new(),
            preview_cache: HashMap::new(),
            buffer_entries: Vec::new(),
            jump_entries: Vec::new(),
            reference_entries: Vec::new(),
            git_branch_entries: Vec::new(),
            symbol_entries,
            symbol_submit_behavior: SymbolSubmitBehavior::CopyToClipboard,
            file_entries: Vec::new(),
            project_root: PathBuf::new(),
            request_tx: None,
            result_rx: None,
            _worker: None,
            requested_paths: HashSet::new(),
            git_status_map: HashMap::new(),
            last_previewed_buffer: None,
            last_previewed_jump_index: None,
            last_previewed_reference_index: None,
            last_previewed_git_branch_index: None,
            last_previewed_symbol_index: None,
            jump_target_preview_line: None,
            jump_target_char_col: None,
            buffer_highlight_cache: HashMap::new(),
            reference_highlight_cache: HashMap::new(),
            lang_registry_owned: None,
            command_history: None,
            global_search_entries: Vec::new(),
            global_search_request_tx: None,
            global_search_result_rx: None,
            _global_search_worker: None,
            global_search_generation: 0,
            global_search_latest_applied: 0,
            global_search_dirty: false,
            global_search_changed_at: None,
            active_doc_lines: Vec::new(),
            is_unified: false,
            caller_label: None,
        };
        palette.update_symbol_preview();
        palette
    }

    pub fn set_input(&mut self, input: String) {
        self.input.set_text(input);
    }

    pub fn new_global_search(
        files: Vec<String>,
        project_root: &Path,
        git_status_map: &HashMap<String, GitFileStatus>,
    ) -> Self {
        let (search_req_tx, search_req_rx) = mpsc::channel::<GlobalSearchRequest>();
        let (search_res_tx, search_res_rx) = mpsc::channel::<GlobalSearchBatch>();
        let root = project_root.to_path_buf();
        let worker_files = files.clone();
        let search_handle = thread::spawn(move || {
            workers::global_search_worker(search_req_rx, search_res_tx, root, worker_files);
        });

        Self {
            input: TextInput::default(),
            mode: PaletteMode::GlobalSearch,
            candidates: Vec::new(),
            selected: 0,
            scroll_offset: 0,
            preview_lines: Vec::new(),
            preview_spans: HashMap::new(),
            preview_cache: HashMap::new(),
            buffer_entries: Vec::new(),
            jump_entries: Vec::new(),
            reference_entries: Vec::new(),
            git_branch_entries: Vec::new(),
            symbol_entries: Vec::new(),
            symbol_submit_behavior: SymbolSubmitBehavior::JumpToLocation,
            file_entries: files,
            project_root: project_root.to_path_buf(),
            request_tx: None,
            result_rx: None,
            _worker: None,
            requested_paths: HashSet::new(),
            git_status_map: git_status_map.clone(),
            last_previewed_buffer: None,
            last_previewed_jump_index: None,
            last_previewed_reference_index: None,
            last_previewed_git_branch_index: None,
            last_previewed_symbol_index: None,
            jump_target_preview_line: None,
            jump_target_char_col: None,
            buffer_highlight_cache: HashMap::new(),
            reference_highlight_cache: HashMap::new(),
            lang_registry_owned: None,
            command_history: None,
            global_search_entries: Vec::new(),
            global_search_request_tx: Some(search_req_tx),
            global_search_result_rx: Some(search_res_rx),
            _global_search_worker: Some(search_handle),
            global_search_generation: 0,
            global_search_latest_applied: 0,
            global_search_dirty: true,
            global_search_changed_at: Some(Instant::now()),
            active_doc_lines: Vec::new(),
            is_unified: false,
            caller_label: None,
        }
    }

    pub fn update_candidates(
        &mut self,
        registry: &CommandRegistry,
        lang_registry: &LanguageRegistry,
        config: &Config,
    ) {
        if self.input.text.starts_with('>') {
            self.mode = PaletteMode::Command;
            let query = self.input.text[1..].trim_start();
            self.candidates =
                Self::filter_commands(registry, query, self.command_history.as_deref(), config);
        } else if self.input.text.starts_with('@') {
            self.mode = PaletteMode::SymbolPicker;
            let query = self.input.text[1..].to_string();
            self.filter_symbol_candidates_with_query(&query);
        } else if self.input.text.starts_with(':') {
            self.mode = PaletteMode::GotoLine;
            self.candidates.clear();
        } else {
            self.mode = PaletteMode::FileFinder;
            self.candidates = Self::filter_files(&self.file_entries, &self.input.text);
        }

        if self.selected >= self.candidates.len() {
            self.selected = self.candidates.len().saturating_sub(1);
        }
        self.clamp_input_cursor();
        self.update_preview(lang_registry, config);
    }

    fn mark_global_search_dirty(&mut self) {
        self.global_search_dirty = true;
        self.global_search_changed_at = Some(Instant::now());
    }

    fn pump_global_search(&mut self) {
        if self.mode != PaletteMode::GlobalSearch {
            return;
        }

        let Some(ref rx) = self.global_search_result_rx else {
            return;
        };

        while let Ok(batch) = rx.try_recv() {
            if batch.generation < self.global_search_latest_applied {
                continue;
            }

            self.global_search_latest_applied = batch.generation;
            if let Some(error) = batch.error {
                self.global_search_entries.clear();
                self.candidates.clear();
                self.selected = 0;
                self.preview_lines = vec![format!("Global search error: {error}")];
                self.preview_spans.clear();
                continue;
            }

            self.global_search_entries = batch.results;
            self.candidates = self
                .global_search_entries
                .iter()
                .enumerate()
                .map(|(i, entry)| {
                    let excerpt = entry
                        .preview_lines
                        .get(1)
                        .map(|s| {
                            s.split_once('|')
                                .map(|(_, right)| right)
                                .unwrap_or(s.as_str())
                        })
                        .unwrap_or("");
                    ScoredCandidate {
                        kind: CandidateKind::SearchResult(i),
                        label: format!("{}:{} {}", entry.rel_path, entry.line + 1, excerpt.trim()),
                        score: 0,
                        match_positions: Vec::new(),
                        preview_lines: entry.preview_lines.clone(),
                    }
                })
                .collect();

            if self.selected >= self.candidates.len() {
                self.selected = self.candidates.len().saturating_sub(1);
            }
            self.preview_lines = self
                .candidates
                .get(self.selected)
                .map(|c| c.preview_lines.clone())
                .unwrap_or_default();
            self.preview_spans.clear();
        }

        if !self.global_search_dirty {
            return;
        }

        let Some(changed_at) = self.global_search_changed_at else {
            return;
        };
        if changed_at.elapsed() < Duration::from_millis(workers::GLOBAL_SEARCH_DEBOUNCE_MS) {
            return;
        }

        let Some(ref tx) = self.global_search_request_tx else {
            return;
        };

        self.global_search_generation = self.global_search_generation.saturating_add(1);
        let request = GlobalSearchRequest {
            query: self.input.text.clone(),
            generation: self.global_search_generation,
        };
        let _ = tx.send(request);
        self.global_search_dirty = false;
    }

    fn filter_commands(
        registry: &CommandRegistry,
        query: &str,
        history: Option<&CommandHistory>,
        config: &Config,
    ) -> Vec<ScoredCandidate> {
        let mut scored: Vec<ScoredCandidate> = registry
            .commands()
            .iter()
            .enumerate()
            .filter_map(|(i, entry)| {
                let display_label = command_display_label(entry, config);
                if query.is_empty() {
                    return Some(ScoredCandidate {
                        kind: CandidateKind::Command(i),
                        label: display_label.clone(),
                        score: 0,
                        match_positions: Vec::new(),
                        preview_lines: command_preview_lines(entry, &display_label),
                    });
                }
                fuzzy_match(&display_label, query).map(|(score, positions)| ScoredCandidate {
                    kind: CandidateKind::Command(i),
                    label: display_label.clone(),
                    score,
                    match_positions: positions,
                    preview_lines: command_preview_lines(entry, &display_label),
                })
            })
            .collect();

        // History-based sorting when query is empty
        if query.is_empty() {
            if let Some(hist) = history {
                let recent_ids = hist.get_recent_commands(100);
                let id_to_rank: HashMap<&str, usize> = recent_ids
                    .iter()
                    .enumerate()
                    .map(|(rank, id)| (id.as_str(), rank))
                    .collect();

                scored.sort_by(|a, b| {
                    let a_idx = match a.kind {
                        CandidateKind::Command(i) => i,
                        _ => return Ordering::Equal,
                    };
                    let b_idx = match b.kind {
                        CandidateKind::Command(i) => i,
                        _ => return Ordering::Equal,
                    };

                    let a_id = &registry.commands()[a_idx].id;
                    let b_id = &registry.commands()[b_idx].id;

                    let a_rank = id_to_rank.get(a_id.as_str());
                    let b_rank = id_to_rank.get(b_id.as_str());

                    match (a_rank, b_rank) {
                        (Some(r1), Some(r2)) => r1.cmp(r2), // Both in history: by recency
                        (Some(_), None) => Ordering::Less,  // a in history, b not
                        (None, Some(_)) => Ordering::Greater, // b in history, a not
                        (None, None) => a.label.cmp(&b.label), // Neither: alphabetical
                    }
                });
            } else {
                // No history: alphabetical fallback
                scored.sort_by(|a, b| a.label.cmp(&b.label));
            }
        } else {
            // With query: fuzzy match score (existing behavior)
            scored.sort_by(|a, b| b.score.cmp(&a.score));
        }

        scored
    }

    fn filter_files(file_entries: &[String], query: &str) -> Vec<ScoredCandidate> {
        if query.is_empty() {
            return file_entries
                .iter()
                .enumerate()
                .map(|(i, path)| ScoredCandidate {
                    kind: CandidateKind::File(i),
                    label: path.clone(),
                    score: 0,
                    match_positions: Vec::new(),
                    preview_lines: Vec::new(),
                })
                .collect();
        }

        let mut scored: Vec<ScoredCandidate> = file_entries
            .iter()
            .enumerate()
            .filter_map(|(i, path)| {
                fuzzy_match(path, query).map(|(score, positions)| ScoredCandidate {
                    kind: CandidateKind::File(i),
                    label: path.clone(),
                    score,
                    match_positions: positions,
                    preview_lines: Vec::new(),
                })
            })
            .collect();

        scored.sort_by(|a, b| b.score.cmp(&a.score));
        scored
    }

    pub fn selected_file_path(&self) -> Option<&str> {
        self.candidates
            .get(self.selected)
            .and_then(|c| match c.kind {
                CandidateKind::File(idx) => Some(self.file_entries[idx].as_str()),
                _ => None,
            })
    }

    pub fn update_preview(&mut self, lang_registry: &LanguageRegistry, config: &Config) {
        let t_total = Instant::now();
        self.preview_lines.clear();
        self.preview_spans.clear();
        self.jump_target_preview_line = None;
        self.jump_target_char_col = None;
        if self.mode == PaletteMode::GlobalSearch {
            self.pump_global_search();
            self.preview_lines = self
                .candidates
                .get(self.selected)
                .map(|c| c.preview_lines.clone())
                .unwrap_or_default();
            return;
        }
        if self.mode == PaletteMode::GotoLine {
            self.update_goto_line_preview();
            return;
        }
        if self.mode == PaletteMode::BufferPicker {
            self.update_buffer_preview();
            return;
        }
        if self.mode == PaletteMode::JumpPicker {
            self.update_jump_preview();
            return;
        }
        if self.mode == PaletteMode::ReferencePicker {
            self.update_reference_preview();
            return;
        }
        if self.mode == PaletteMode::GitBranchPicker {
            self.update_git_branch_preview();
            return;
        }
        if self.mode == PaletteMode::SymbolPicker {
            self.update_symbol_preview();
            return;
        }
        if self.mode == PaletteMode::Command {
            self.preview_lines = self
                .candidates
                .get(self.selected)
                .map(|c| c.preview_lines.clone())
                .unwrap_or_default();
            return;
        }
        self.last_previewed_buffer = None;
        self.last_previewed_jump_index = None;
        self.last_previewed_reference_index = None;
        self.last_previewed_git_branch_index = None;
        self.last_previewed_symbol_index = None;
        if self.mode != PaletteMode::FileFinder {
            return;
        }

        // Drain any completed background results into cache
        self.drain_preview_results();

        if let Some(rel_path) = self
            .candidates
            .get(self.selected)
            .and_then(|c| match c.kind {
                CandidateKind::File(idx) => Some(&self.file_entries[idx]),
                _ => None,
            })
        {
            let rel_path = rel_path.clone();

            // Check cache first (includes background-generated results)
            if let Some(cached) = self.preview_cache.get(&rel_path) {
                self.preview_lines = cached.0.clone();
                self.preview_spans = cached.1.clone();
                debug_log!(
                    config,
                    "preview: file={} cache hit total={}µs",
                    rel_path,
                    t_total.elapsed().as_micros()
                );
                self.schedule_nearby_previews(config);
                return;
            }

            // Sync fallback: generate preview on main thread for current selection
            let full_path = self.project_root.join(&rel_path);

            let t_read = Instant::now();
            if let Ok(content) = std::fs::read_to_string(&full_path) {
                let read_us = t_read.elapsed().as_micros();

                self.preview_lines = content.lines().take(200).map(|l| l.to_string()).collect();

                if let Some(lang_def) = lang_registry.detect_by_extension(&rel_path) {
                    let preview_text: String = self.preview_lines.join("\n");

                    let t_hl = Instant::now();
                    self.preview_spans = highlight_text(&preview_text, lang_def);
                    let hl_us = t_hl.elapsed().as_micros();

                    debug_log!(
                        config,
                        "preview: file={} size={} read={}µs highlight={}µs total={}µs (sync fallback)",
                        rel_path,
                        content.len(),
                        read_us,
                        hl_us,
                        t_total.elapsed().as_micros()
                    );
                } else {
                    debug_log!(
                        config,
                        "preview: file={} size={} read={}µs (no lang) total={}µs (sync fallback)",
                        rel_path,
                        content.len(),
                        read_us,
                        t_total.elapsed().as_micros()
                    );
                }

                // Store in cache
                self.preview_cache.insert(
                    rel_path,
                    (self.preview_lines.clone(), self.preview_spans.clone()),
                );
            }
        }

        // Schedule nearby previews for background generation
        self.schedule_nearby_previews(config);
    }

    fn drain_preview_results(&mut self) {
        let Some(ref rx) = self.result_rx else { return };
        while let Ok(result) = rx.try_recv() {
            self.preview_cache
                .insert(result.rel_path, (result.lines, result.spans));
        }
    }

    fn schedule_nearby_previews(&mut self, config: &Config) {
        let Some(ref tx) = self.request_tx else {
            return;
        };
        if self.candidates.is_empty() {
            return;
        }

        let start = self.selected.saturating_sub(5);
        let end = (self.selected + 15).min(self.candidates.len());
        let mut count = 0;

        for idx in start..end {
            if let Some(rel_path) = self.candidates.get(idx).and_then(|c| match c.kind {
                CandidateKind::File(i) => Some(&self.file_entries[i]),
                _ => None,
            }) {
                let rel_path = rel_path.clone();
                if self.preview_cache.contains_key(&rel_path)
                    || self.requested_paths.contains(&rel_path)
                {
                    continue;
                }
                self.requested_paths.insert(rel_path.clone());
                if tx.send(PreviewRequest { rel_path }).is_err() {
                    break;
                }
                count += 1;
            }
        }

        if count > 0 {
            debug_log!(config, "preview: scheduled {} nearby previews", count);
        }
    }

    fn filter_buffer_candidates(&mut self) {
        let query = &self.input.text;
        if query.is_empty() {
            self.candidates = self
                .buffer_entries
                .iter()
                .map(|(id, name, _)| ScoredCandidate {
                    kind: CandidateKind::Buffer(*id),
                    label: name.clone(),
                    score: 0,
                    match_positions: Vec::new(),
                    preview_lines: Vec::new(),
                })
                .collect();
        } else {
            let mut scored: Vec<ScoredCandidate> = self
                .buffer_entries
                .iter()
                .filter_map(|(id, name, _)| {
                    fuzzy_match(name, query).map(|(score, positions)| ScoredCandidate {
                        kind: CandidateKind::Buffer(*id),
                        label: name.clone(),
                        score,
                        match_positions: positions,
                        preview_lines: Vec::new(),
                    })
                })
                .collect();
            scored.sort_by(|a, b| b.score.cmp(&a.score));
            self.candidates = scored;
        }

        if self.selected >= self.candidates.len() {
            self.selected = self.candidates.len().saturating_sub(1);
        }
    }

    fn filter_jump_candidates(&mut self) {
        let query = &self.input.text;
        if query.is_empty() {
            self.candidates = self
                .jump_entries
                .iter()
                .map(|entry| ScoredCandidate {
                    kind: CandidateKind::Jump(entry.jump_index),
                    label: entry.label.clone(),
                    score: 0,
                    match_positions: Vec::new(),
                    preview_lines: Vec::new(),
                })
                .collect();
        } else {
            let mut scored: Vec<ScoredCandidate> = self
                .jump_entries
                .iter()
                .filter_map(|entry| {
                    fzf_style_match(&entry.label, query).map(|(score, positions)| ScoredCandidate {
                        kind: CandidateKind::Jump(entry.jump_index),
                        label: entry.label.clone(),
                        score,
                        match_positions: positions,
                        preview_lines: Vec::new(),
                    })
                })
                .collect();
            scored.sort_by(|a, b| b.score.cmp(&a.score));
            self.candidates = scored;
        }

        if self.selected >= self.candidates.len() {
            self.selected = self.candidates.len().saturating_sub(1);
        }
    }

    fn filter_reference_candidates(&mut self) {
        let query = &self.input.text;
        if query.is_empty() {
            self.candidates = self
                .reference_entries
                .iter()
                .enumerate()
                .map(|(idx, entry)| ScoredCandidate {
                    kind: CandidateKind::Reference(idx),
                    label: entry.label.clone(),
                    score: 0,
                    match_positions: Vec::new(),
                    preview_lines: Vec::new(),
                })
                .collect();
        } else {
            let mut scored: Vec<ScoredCandidate> = self
                .reference_entries
                .iter()
                .enumerate()
                .filter_map(|(idx, entry)| {
                    fzf_style_match(&entry.label, query).map(|(score, positions)| ScoredCandidate {
                        kind: CandidateKind::Reference(idx),
                        label: entry.label.clone(),
                        score,
                        match_positions: positions,
                        preview_lines: Vec::new(),
                    })
                })
                .collect();
            scored.sort_by(|a, b| b.score.cmp(&a.score));
            self.candidates = scored;
        }

        if self.selected >= self.candidates.len() {
            self.selected = self.candidates.len().saturating_sub(1);
        }
    }

    fn filter_git_branch_candidates(&mut self) {
        let query = &self.input.text;
        if query.is_empty() {
            self.candidates = self
                .git_branch_entries
                .iter()
                .enumerate()
                .map(|(idx, entry)| ScoredCandidate {
                    kind: CandidateKind::GitBranch(idx),
                    label: entry.label.clone(),
                    score: 0,
                    match_positions: Vec::new(),
                    preview_lines: Vec::new(),
                })
                .collect();
        } else {
            let mut scored: Vec<ScoredCandidate> = self
                .git_branch_entries
                .iter()
                .enumerate()
                .filter_map(|(idx, entry)| {
                    fzf_style_match(&entry.label, query).map(|(score, positions)| ScoredCandidate {
                        kind: CandidateKind::GitBranch(idx),
                        label: entry.label.clone(),
                        score,
                        match_positions: positions,
                        preview_lines: Vec::new(),
                    })
                })
                .collect();
            scored.sort_by(|a, b| b.score.cmp(&a.score));
            self.candidates = scored;
        }

        if self.selected >= self.candidates.len() {
            self.selected = self.candidates.len().saturating_sub(1);
        }
    }

    fn filter_symbol_candidates(&mut self) {
        let query = self.input.text.clone();
        self.filter_symbol_candidates_with_query(&query);
    }

    fn filter_symbol_candidates_with_query(&mut self, query: &str) {
        if query.is_empty() {
            self.candidates = self
                .symbol_entries
                .iter()
                .enumerate()
                .map(|(idx, entry)| ScoredCandidate {
                    kind: CandidateKind::Symbol(idx),
                    label: entry.label.clone(),
                    score: 0,
                    match_positions: Vec::new(),
                    preview_lines: Vec::new(),
                })
                .collect();
        } else {
            let mut scored: Vec<ScoredCandidate> = self
                .symbol_entries
                .iter()
                .enumerate()
                .filter_map(|(idx, entry)| {
                    fzf_style_match(&entry.label, query).map(|(score, positions)| ScoredCandidate {
                        kind: CandidateKind::Symbol(idx),
                        label: entry.label.clone(),
                        score,
                        match_positions: positions,
                        preview_lines: Vec::new(),
                    })
                })
                .collect();
            scored.sort_by(|a, b| b.score.cmp(&a.score));
            self.candidates = scored;
        }

        if self.selected >= self.candidates.len() {
            self.selected = self.candidates.len().saturating_sub(1);
        }
    }

    fn min_input_cursor(&self) -> usize {
        if self.is_unified {
            return 0;
        }
        match self.input.text.chars().next() {
            Some('>') | Some('@') | Some(':') => 1,
            _ => 0,
        }
    }

    fn clamp_input_cursor(&mut self) {
        self.input.min_cursor = self.min_input_cursor();
        self.input.clamp();
    }

    fn refresh_after_input_edit(
        &mut self,
        registry: &CommandRegistry,
        lang_registry: &LanguageRegistry,
        config: &Config,
    ) {
        self.clamp_input_cursor();
        if self.is_unified {
            self.update_candidates(registry, lang_registry, config);
            return;
        }
        match self.mode {
            PaletteMode::BufferPicker => {
                self.filter_buffer_candidates();
                self.update_buffer_preview();
            }
            PaletteMode::JumpPicker => {
                self.filter_jump_candidates();
                self.update_jump_preview();
            }
            PaletteMode::ReferencePicker => {
                self.filter_reference_candidates();
                self.update_reference_preview();
            }
            PaletteMode::GitBranchPicker => {
                self.filter_git_branch_candidates();
                self.update_git_branch_preview();
            }
            PaletteMode::SymbolPicker => {
                self.filter_symbol_candidates();
                self.update_symbol_preview();
            }
            PaletteMode::GlobalSearch => {
                self.mark_global_search_dirty();
                self.pump_global_search();
            }
            _ => self.update_candidates(registry, lang_registry, config),
        }
    }

    pub fn on_char(
        &mut self,
        c: char,
        registry: &CommandRegistry,
        lang_registry: &LanguageRegistry,
        config: &Config,
    ) {
        self.clamp_input_cursor();
        self.input.insert_char(c);
        self.refresh_after_input_edit(registry, lang_registry, config);
    }

    /// Insert a string (e.g., from a Paste event or IME composition) into the input.
    pub fn insert_text(
        &mut self,
        text: &str,
        registry: &CommandRegistry,
        lang_registry: &LanguageRegistry,
        config: &Config,
    ) {
        self.clamp_input_cursor();
        for c in text.chars() {
            self.input.insert_char(c);
        }
        self.refresh_after_input_edit(registry, lang_registry, config);
    }

    pub fn on_backspace(
        &mut self,
        registry: &CommandRegistry,
        lang_registry: &LanguageRegistry,
        config: &Config,
    ) {
        self.clamp_input_cursor();
        if self.input.backspace() {
            self.refresh_after_input_edit(registry, lang_registry, config);
        }
    }

    pub fn on_char_buffer(&mut self, c: char) {
        self.clamp_input_cursor();
        self.input.insert_char(c);
        self.filter_buffer_candidates();
        self.update_buffer_preview();
    }

    pub fn on_backspace_buffer(&mut self) {
        self.clamp_input_cursor();
        if self.input.backspace() {
            self.filter_buffer_candidates();
            self.update_buffer_preview();
        }
    }

    pub fn on_char_jump(&mut self, c: char) {
        self.clamp_input_cursor();
        self.input.insert_char(c);
        self.filter_jump_candidates();
        self.update_jump_preview();
    }

    pub fn on_backspace_jump(&mut self) {
        self.clamp_input_cursor();
        if self.input.backspace() {
            self.filter_jump_candidates();
            self.update_jump_preview();
        }
    }

    pub fn on_delete_prev_word(
        &mut self,
        registry: &CommandRegistry,
        lang_registry: &LanguageRegistry,
        config: &Config,
    ) {
        self.clamp_input_cursor();
        if self.input.delete_prev_word() {
            self.refresh_after_input_edit(registry, lang_registry, config);
        }
    }

    pub fn on_delete_to_end(
        &mut self,
        registry: &CommandRegistry,
        lang_registry: &LanguageRegistry,
        config: &Config,
    ) {
        self.clamp_input_cursor();
        if self.input.delete_to_end() {
            self.refresh_after_input_edit(registry, lang_registry, config);
        }
    }

    fn update_buffer_preview(&mut self) {
        self.jump_target_preview_line = None;
        self.jump_target_char_col = None;
        let selected_id = match self.candidates.get(self.selected) {
            Some(c) => match c.kind {
                CandidateKind::Buffer(id) => id,
                _ => {
                    self.preview_lines.clear();
                    self.preview_spans.clear();
                    self.last_previewed_buffer = None;
                    return;
                }
            },
            None => {
                self.preview_lines.clear();
                self.preview_spans.clear();
                self.last_previewed_buffer = None;
                return;
            }
        };

        // Skip redundant work if the selected buffer hasn't changed
        if self.last_previewed_buffer == Some(selected_id) {
            return;
        }

        let entry = self
            .buffer_entries
            .iter()
            .find(|(id, _, _)| *id == selected_id);
        let (_, name, lines) = match entry {
            Some(e) => e,
            None => {
                self.preview_lines.clear();
                self.preview_spans.clear();
                self.last_previewed_buffer = None;
                return;
            }
        };

        self.preview_lines = lines.clone();

        // Check highlight cache first
        if let Some(cached_spans) = self.buffer_highlight_cache.get(&selected_id) {
            self.preview_spans = cached_spans.clone();
        } else {
            let lang_registry = self
                .lang_registry_owned
                .get_or_insert_with(LanguageRegistry::new);
            if let Some(lang_def) = lang_registry.detect_by_extension(name) {
                let preview_text: String = self.preview_lines.join("\n");
                let spans = highlight_text(&preview_text, lang_def);
                self.buffer_highlight_cache
                    .insert(selected_id, spans.clone());
                self.preview_spans = spans;
            } else {
                self.preview_spans.clear();
            }
        }

        self.last_previewed_buffer = Some(selected_id);
    }

    fn update_jump_preview(&mut self) {
        let selected_jump_index = match self.candidates.get(self.selected) {
            Some(c) => match c.kind {
                CandidateKind::Jump(idx) => idx,
                _ => {
                    self.preview_lines.clear();
                    self.preview_spans.clear();
                    self.last_previewed_jump_index = None;
                    self.jump_target_preview_line = None;
                    self.jump_target_char_col = None;
                    return;
                }
            },
            None => {
                self.preview_lines.clear();
                self.preview_spans.clear();
                self.last_previewed_jump_index = None;
                self.jump_target_preview_line = None;
                self.jump_target_char_col = None;
                return;
            }
        };

        if self.last_previewed_jump_index == Some(selected_jump_index) {
            return;
        }

        if let Some(entry) = self
            .jump_entries
            .iter()
            .find(|entry| entry.jump_index == selected_jump_index)
        {
            self.preview_lines = entry.preview_lines.clone();
            self.jump_target_preview_line = entry.target_preview_line;
            self.jump_target_char_col = Some(entry.target_char_col);
            self.preview_spans.clear();
            if let Some(source_path) = entry.source_path.as_deref() {
                let lang_registry = self
                    .lang_registry_owned
                    .get_or_insert_with(LanguageRegistry::new);
                if let Some(lang_def) = lang_registry.detect_by_extension(source_path) {
                    let mut code_lines = Vec::new();
                    let mut line_map: Vec<(usize, usize)> = Vec::new();
                    for (preview_idx, line) in self.preview_lines.iter().enumerate().skip(1) {
                        if let Some((_, code)) = split_numbered_preview_line(line) {
                            let prefix_len = line.len().saturating_sub(code.len());
                            code_lines.push(code.to_string());
                            line_map.push((preview_idx, prefix_len));
                        } else {
                            code_lines.push(line.clone());
                            line_map.push((preview_idx, 0));
                        }
                    }
                    if !code_lines.is_empty() {
                        let preview_text = code_lines.join("\n");
                        let raw_spans = highlight_text(&preview_text, lang_def);
                        for (line_idx, spans) in raw_spans {
                            if let Some((preview_idx, prefix_len)) = line_map.get(line_idx).copied()
                            {
                                self.preview_spans.insert(
                                    preview_idx,
                                    spans
                                        .into_iter()
                                        .map(|span| HighlightSpan {
                                            start: span.start + prefix_len,
                                            end: span.end + prefix_len,
                                            capture_name: span.capture_name,
                                        })
                                        .collect(),
                                );
                            }
                        }
                    }
                }
            }
            self.last_previewed_jump_index = Some(selected_jump_index);
        } else {
            self.preview_lines.clear();
            self.preview_spans.clear();
            self.last_previewed_jump_index = None;
            self.jump_target_preview_line = None;
            self.jump_target_char_col = None;
        }
    }

    fn update_reference_preview(&mut self) {
        let selected_reference_index = match self.candidates.get(self.selected) {
            Some(c) => match c.kind {
                CandidateKind::Reference(idx) => idx,
                _ => {
                    self.preview_lines.clear();
                    self.preview_spans.clear();
                    self.last_previewed_reference_index = None;
                    self.jump_target_preview_line = None;
                    self.jump_target_char_col = None;
                    return;
                }
            },
            None => {
                self.preview_lines.clear();
                self.preview_spans.clear();
                self.last_previewed_reference_index = None;
                self.jump_target_preview_line = None;
                self.jump_target_char_col = None;
                return;
            }
        };

        if self.last_previewed_reference_index == Some(selected_reference_index) {
            return;
        }

        if let Some(entry) = self.reference_entries.get(selected_reference_index) {
            self.preview_lines = entry.preview_lines.clone();
            self.jump_target_preview_line = entry.target_preview_line;
            self.jump_target_char_col = Some(entry.target_char_col);
            if let Some(cached_spans) = self
                .reference_highlight_cache
                .get(&selected_reference_index)
            {
                self.preview_spans = cached_spans.clone();
            } else {
                self.preview_spans.clear();
                if let Some(source_path) = entry.source_path.as_deref() {
                    let lang_registry = self
                        .lang_registry_owned
                        .get_or_insert_with(LanguageRegistry::new);
                    if let Some(lang_def) = lang_registry.detect_by_extension(source_path) {
                        let mut code_lines = Vec::new();
                        let mut line_map: Vec<(usize, usize)> = Vec::new();
                        for (preview_idx, line) in self.preview_lines.iter().enumerate().skip(1) {
                            if let Some((_, code)) = split_numbered_preview_line(line) {
                                let prefix_len = line.len().saturating_sub(code.len());
                                code_lines.push(code.to_string());
                                line_map.push((preview_idx, prefix_len));
                            } else {
                                code_lines.push(line.clone());
                                line_map.push((preview_idx, 0));
                            }
                        }
                        if !code_lines.is_empty() {
                            let preview_text = code_lines.join("\n");
                            let raw_spans = highlight_text(&preview_text, lang_def);
                            for (line_idx, spans) in raw_spans {
                                if let Some((preview_idx, prefix_len)) =
                                    line_map.get(line_idx).copied()
                                {
                                    self.preview_spans.insert(
                                        preview_idx,
                                        spans
                                            .into_iter()
                                            .map(|span| HighlightSpan {
                                                start: span.start + prefix_len,
                                                end: span.end + prefix_len,
                                                capture_name: span.capture_name,
                                            })
                                            .collect(),
                                    );
                                }
                            }
                        }
                    }
                }
                self.reference_highlight_cache
                    .insert(selected_reference_index, self.preview_spans.clone());
            }
            self.last_previewed_reference_index = Some(selected_reference_index);
        } else {
            self.preview_lines.clear();
            self.preview_spans.clear();
            self.last_previewed_reference_index = None;
            self.jump_target_preview_line = None;
            self.jump_target_char_col = None;
        }
    }

    fn update_git_branch_preview(&mut self) {
        self.jump_target_preview_line = None;
        self.jump_target_char_col = None;
        let selected_branch_index = match self.candidates.get(self.selected) {
            Some(c) => match c.kind {
                CandidateKind::GitBranch(idx) => idx,
                _ => {
                    self.preview_lines.clear();
                    self.preview_spans.clear();
                    self.last_previewed_git_branch_index = None;
                    return;
                }
            },
            None => {
                self.preview_lines.clear();
                self.preview_spans.clear();
                self.last_previewed_git_branch_index = None;
                return;
            }
        };

        if self.last_previewed_git_branch_index == Some(selected_branch_index) {
            return;
        }

        if let Some(entry) = self.git_branch_entries.get(selected_branch_index) {
            self.preview_lines = entry.preview_lines.clone();
            self.preview_spans.clear();
            self.last_previewed_git_branch_index = Some(selected_branch_index);
        } else {
            self.preview_lines.clear();
            self.preview_spans.clear();
            self.last_previewed_git_branch_index = None;
        }
    }

    fn update_symbol_preview(&mut self) {
        self.jump_target_preview_line = None;
        self.jump_target_char_col = None;
        let selected_symbol_index = match self.candidates.get(self.selected) {
            Some(c) => match c.kind {
                CandidateKind::Symbol(idx) => idx,
                _ => {
                    self.preview_lines.clear();
                    self.preview_spans.clear();
                    self.last_previewed_symbol_index = None;
                    return;
                }
            },
            None => {
                self.preview_lines.clear();
                self.preview_spans.clear();
                self.last_previewed_symbol_index = None;
                return;
            }
        };

        if self.last_previewed_symbol_index == Some(selected_symbol_index) {
            return;
        }

        if let Some(entry) = self.symbol_entries.get(selected_symbol_index) {
            self.preview_lines = entry.preview_lines.clone();
            self.preview_spans.clear();
            self.jump_target_preview_line =
                self.preview_lines
                    .iter()
                    .enumerate()
                    .find_map(|(preview_idx, line)| {
                        let (prefix, _) = split_numbered_preview_line(line)?;
                        let line_no = prefix.trim().parse::<usize>().ok()?;
                        (line_no == entry.line + 1).then_some(preview_idx)
                    });
            self.jump_target_char_col = Some(entry.char_col);
            self.last_previewed_symbol_index = Some(selected_symbol_index);
        } else {
            self.preview_lines.clear();
            self.preview_spans.clear();
            self.last_previewed_symbol_index = None;
            self.jump_target_preview_line = None;
            self.jump_target_char_col = None;
        }
    }

    fn update_goto_line_preview(&mut self) {
        self.preview_lines.clear();
        self.preview_spans.clear();
        self.jump_target_preview_line = None;
        self.jump_target_char_col = None;

        let line_str = self.input.text[1..].trim();
        let target_line = match line_str.parse::<usize>() {
            Ok(n) if n > 0 => n - 1,
            _ => return,
        };

        let total = self.active_doc_lines.len();
        if total == 0 {
            return;
        }
        let target_line = target_line.min(total.saturating_sub(1));
        let start = target_line.saturating_sub(5);
        let end = (target_line + 6).min(total);

        for line_idx in start..end {
            let text = &self.active_doc_lines[line_idx];
            self.preview_lines
                .push(format!("{:>5} | {}", line_idx + 1, text));
        }

        self.jump_target_preview_line = Some(target_line - start);
        self.jump_target_char_col = Some(0);
    }

    pub fn select_next(&mut self, lang_registry: &LanguageRegistry, config: &Config) {
        if !self.candidates.is_empty() {
            let prev = self.selected;
            self.selected = if self.selected + 1 >= self.candidates.len() {
                0
            } else {
                self.selected + 1
            };
            if self.selected != prev {
                self.update_preview(lang_registry, config);
            }
        }
    }

    pub fn select_prev(&mut self, lang_registry: &LanguageRegistry, config: &Config) {
        if !self.candidates.is_empty() {
            let prev = self.selected;
            self.selected = if self.selected == 0 {
                self.candidates.len() - 1
            } else {
                self.selected - 1
            };
            if self.selected != prev {
                self.update_preview(lang_registry, config);
            }
        }
    }

    pub fn ensure_selection_visible(&mut self, visible_count: usize) {
        if visible_count == 0 {
            return;
        }
        if self.selected < self.scroll_offset {
            self.scroll_offset = self.selected;
        } else if self.selected >= self.scroll_offset + visible_count {
            self.scroll_offset = self.selected - visible_count + 1;
        }
    }

    pub fn selected_command_index(&self) -> Option<usize> {
        self.candidates
            .get(self.selected)
            .and_then(|c| match c.kind {
                CandidateKind::Command(idx) => Some(idx),
                _ => None,
            })
    }

    pub fn selected_buffer_id(&self) -> Option<BufferId> {
        self.candidates
            .get(self.selected)
            .and_then(|c| match c.kind {
                CandidateKind::Buffer(id) => Some(id),
                _ => None,
            })
    }

    pub fn selected_jump_index(&self) -> Option<usize> {
        self.candidates
            .get(self.selected)
            .and_then(|c| match c.kind {
                CandidateKind::Jump(idx) => Some(idx),
                _ => None,
            })
    }

    pub fn selected_reference_location(&self) -> Option<(PathBuf, usize, usize)> {
        self.candidates
            .get(self.selected)
            .and_then(|c| match c.kind {
                CandidateKind::Reference(idx) => self
                    .reference_entries
                    .get(idx)
                    .map(|entry| (entry.path.clone(), entry.line, entry.character_utf16)),
                _ => None,
            })
    }

    pub fn selected_git_branch(&self) -> Option<String> {
        self.candidates
            .get(self.selected)
            .and_then(|c| match c.kind {
                CandidateKind::GitBranch(idx) => self
                    .git_branch_entries
                    .get(idx)
                    .map(|entry| entry.branch_name.clone()),
                _ => None,
            })
    }

    pub fn selected_symbol_location(&self) -> Option<(usize, usize)> {
        self.candidates
            .get(self.selected)
            .and_then(|c| match c.kind {
                CandidateKind::Symbol(idx) => self
                    .symbol_entries
                    .get(idx)
                    .map(|entry| (entry.line, entry.char_col)),
                _ => None,
            })
    }

    fn selected_symbol_copy_text(&self) -> Option<String> {
        self.candidates
            .get(self.selected)
            .and_then(|c| match c.kind {
                CandidateKind::Symbol(idx) => self
                    .symbol_entries
                    .get(idx)
                    .and_then(|entry| entry.copy_text.clone()),
                _ => None,
            })
    }

    fn selected_search_result(&self) -> Option<&GlobalSearchResultEntry> {
        self.candidates
            .get(self.selected)
            .and_then(|c| match c.kind {
                CandidateKind::SearchResult(idx) => self.global_search_entries.get(idx),
                _ => None,
            })
    }

    /// Parse `:LINE` or `:LINE:CHAR` input into 0-based (line, char_col).
    fn parse_goto_line(input: &str) -> Option<(usize, usize)> {
        let text = input.strip_prefix(':')?.trim();
        if text.is_empty() {
            return None;
        }
        let parts: Vec<&str> = text.splitn(2, ':').collect();
        let line = parts[0].trim().parse::<usize>().ok()?;
        let char_col = if parts.len() > 1 {
            parts[1].trim().parse::<usize>().unwrap_or(1)
        } else {
            1
        };
        Some((line.saturating_sub(1), char_col.saturating_sub(1)))
    }

    /// Handle a key event and return an EventResult.
    /// The registry is needed for command/file filtering.
    pub fn handle_key_event(
        &mut self,
        key: KeyEvent,
        registry: &CommandRegistry,
        lang_registry: &LanguageRegistry,
        config: &Config,
    ) -> EventResult {
        self.pump_global_search();

        debug_log!(
            config,
            "palette: key={:?}, kind={:?}, input_before={:?}",
            key.code,
            key.kind,
            self.input.text
        );

        // Ignore non-Press events (e.g., Release). This is critical for IME input:
        // when the user presses Enter to confirm IME composition, we must ignore
        // the Release event to avoid clearing the input prematurely.
        if key.kind != KeyEventKind::Press {
            debug_log!(config, "palette: ignoring non-Press event");
            return EventResult::Consumed;
        }

        // Control-key bindings for picker navigation and query editing
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('n') | KeyCode::Char('j') => {
                    self.select_next(lang_registry, config);
                    return EventResult::Consumed;
                }
                KeyCode::Char('p') => {
                    self.select_prev(lang_registry, config);
                    return EventResult::Consumed;
                }
                KeyCode::Left => {
                    self.clamp_input_cursor();
                    self.input.move_word_left();
                    return EventResult::Consumed;
                }
                KeyCode::Right => {
                    self.clamp_input_cursor();
                    self.input.move_word_right();
                    return EventResult::Consumed;
                }
                KeyCode::Char('f') => {
                    self.clamp_input_cursor();
                    self.input.move_right();
                    return EventResult::Consumed;
                }
                KeyCode::Char('b') => {
                    self.clamp_input_cursor();
                    self.input.move_left();
                    return EventResult::Consumed;
                }
                KeyCode::Char('a') => {
                    self.clamp_input_cursor();
                    self.input.move_start();
                    return EventResult::Consumed;
                }
                KeyCode::Char('e') => {
                    self.clamp_input_cursor();
                    self.input.move_end();
                    return EventResult::Consumed;
                }
                KeyCode::Char('w') => {
                    self.on_delete_prev_word(registry, lang_registry, config);
                    return EventResult::Consumed;
                }
                KeyCode::Char('k') => {
                    self.on_delete_to_end(registry, lang_registry, config);
                    return EventResult::Consumed;
                }
                KeyCode::Char('c') | KeyCode::Char('q') => {
                    return EventResult::Action(Action::Ui(UiAction::ClosePalette));
                }
                _ => {}
            }
        }

        match key.code {
            KeyCode::Esc => EventResult::Action(Action::Ui(UiAction::ClosePalette)),
            KeyCode::Down => {
                self.select_next(lang_registry, config);
                EventResult::Consumed
            }
            KeyCode::Up => {
                self.select_prev(lang_registry, config);
                EventResult::Consumed
            }
            KeyCode::Left => {
                self.clamp_input_cursor();
                self.input.move_left();
                EventResult::Consumed
            }
            KeyCode::Right => {
                self.clamp_input_cursor();
                self.input.move_right();
                EventResult::Consumed
            }
            KeyCode::Backspace => {
                self.on_backspace(registry, lang_registry, config);
                EventResult::Consumed
            }
            KeyCode::Enter => {
                debug_log!(
                    config,
                    "palette: Enter pressed, mode={:?}, input={:?}, candidates={}",
                    self.mode,
                    self.input.text,
                    self.candidates.len()
                );
                // Determine action based on mode and selection
                match self.mode {
                    PaletteMode::BufferPicker => {
                        if let Some(buf_id) = self.selected_buffer_id() {
                            EventResult::Action(Action::App(AppAction::Buffer(
                                BufferAction::SwitchBufferById(buf_id),
                            )))
                        } else {
                            EventResult::Action(Action::Ui(UiAction::ClosePalette))
                        }
                    }
                    PaletteMode::JumpPicker => {
                        if let Some(idx) = self.selected_jump_index() {
                            EventResult::Action(Action::App(AppAction::Navigation(
                                NavigationAction::JumpToListIndex(idx),
                            )))
                        } else {
                            EventResult::Action(Action::Ui(UiAction::ClosePalette))
                        }
                    }
                    PaletteMode::ReferencePicker => {
                        if let Some((path, line, character_utf16)) =
                            self.selected_reference_location()
                        {
                            EventResult::Action(Action::App(AppAction::Navigation(
                                NavigationAction::OpenFileAtLspLocation {
                                    path,
                                    line,
                                    character_utf16,
                                },
                            )))
                        } else {
                            EventResult::Action(Action::Ui(UiAction::ClosePalette))
                        }
                    }
                    PaletteMode::GitBranchPicker => {
                        if let Some(branch) = self.selected_git_branch() {
                            EventResult::Action(Action::App(AppAction::Project(
                                ProjectAction::SwitchGitBranch(branch),
                            )))
                        } else {
                            EventResult::Action(Action::Ui(UiAction::ClosePalette))
                        }
                    }
                    PaletteMode::SymbolPicker => match self.symbol_submit_behavior {
                        SymbolSubmitBehavior::JumpToLocation => {
                            if let Some((line, char_col)) = self.selected_symbol_location() {
                                EventResult::Action(Action::App(AppAction::Navigation(
                                    NavigationAction::JumpToLineChar { line, char_col },
                                )))
                            } else {
                                EventResult::Action(Action::Ui(UiAction::ClosePalette))
                            }
                        }
                        SymbolSubmitBehavior::CopyToClipboard => {
                            if let Some(text) = self.selected_symbol_copy_text() {
                                EventResult::Action(Action::App(AppAction::Integration(
                                    IntegrationAction::CopyToClipboard {
                                        text,
                                        description: "smart copy section".to_string(),
                                    },
                                )))
                            } else {
                                EventResult::Action(Action::Ui(UiAction::ClosePalette))
                            }
                        }
                    },
                    PaletteMode::FileFinder => {
                        if let Some(path) = self.selected_file_path() {
                            EventResult::Action(Action::App(AppAction::Buffer(
                                BufferAction::OpenProjectFile(path.to_string()),
                            )))
                        } else {
                            EventResult::Action(Action::Ui(UiAction::ClosePalette))
                        }
                    }
                    PaletteMode::Command => {
                        if let Some(idx) = self.selected_command_index() {
                            EventResult::Action(Action::App(AppAction::Navigation(
                                NavigationAction::ExecutePaletteCommand(idx),
                            )))
                        } else {
                            EventResult::Action(Action::Ui(UiAction::ClosePalette))
                        }
                    }
                    PaletteMode::GlobalSearch => {
                        if let Some(entry) = self.selected_search_result() {
                            EventResult::Action(Action::App(AppAction::Buffer(
                                BufferAction::OpenProjectFileAt {
                                    rel_path: entry.rel_path.clone(),
                                    line: entry.line,
                                    char_col: entry.char_col,
                                },
                            )))
                        } else {
                            EventResult::Action(Action::Ui(UiAction::ClosePalette))
                        }
                    }
                    PaletteMode::GotoLine => {
                        if let Some((line, char_col)) = Self::parse_goto_line(&self.input.text) {
                            EventResult::Action(Action::App(AppAction::Navigation(
                                NavigationAction::JumpToLineChar { line, char_col },
                            )))
                        } else {
                            EventResult::Action(Action::Ui(UiAction::ClosePalette))
                        }
                    }
                }
            }
            KeyCode::Char(c) => {
                self.on_char(c, registry, lang_registry, config);
                EventResult::Consumed
            }
            _ => EventResult::Consumed,
        }
    }

    /// Render the palette overlay onto a Surface. Returns (cursor_x, cursor_y) for the input field.
    pub fn render_overlay(&mut self, surface: &mut Surface, theme: &Theme) -> (u16, u16) {
        self.pump_global_search();

        let cols = surface.width;
        let rows = surface.height;
        let popup_w = (cols * 80 / 100).max(3);
        let popup_h = (rows * 80 / 100).max(3);
        let offset_x = (cols.saturating_sub(popup_w)) / 2;
        let offset_y = (rows.saturating_sub(popup_h)) / 2;

        let split_threshold = 60;
        let left_x = offset_x;
        let left_w;

        if popup_w >= split_threshold {
            let gap = 2;
            left_w = (popup_w - gap) / 2;
            let right_w = popup_w - gap - left_w;
            let right_x = offset_x + left_w + gap;

            self.render_left_panel(surface, left_x, offset_y, left_w, popup_h);
            self.render_right_panel(surface, right_x, offset_y, right_w, popup_h, theme);
        } else {
            left_w = popup_w;
            self.render_left_panel(surface, left_x, offset_y, left_w, popup_h);
        }

        // Cursor position in input field
        let prompt = " ";
        let inner_w = left_w.saturating_sub(2);
        let max_input_w = inner_w.saturating_sub(prompt.len());
        let cursor_byte = self.input.byte_index_at_cursor();
        let cursor_slice = &self.input.text[..cursor_byte];
        let (_, visible_w) = truncate_to_width(cursor_slice, max_input_w);

        let cursor_x = (left_x + 1 + prompt.len() + visible_w) as u16;
        let cursor_y = (offset_y + 1) as u16;

        (cursor_x, cursor_y)
    }

    fn render_left_panel(&mut self, surface: &mut Surface, x: usize, y: usize, w: usize, h: usize) {
        let inner_w = w.saturating_sub(2);
        let candidate_area_h = h.saturating_sub(4);
        let default_style = CellStyle::default();

        self.ensure_selection_visible(candidate_area_h);

        for row in 0..h {
            if row == 0 {
                surface.put_str(x, y + row, "┌", &default_style);
                surface.fill_region(x + 1, y + row, inner_w, '─', &default_style);
                if let Some(caller_label) = self.caller_label.as_deref() {
                    let label_text = format!(" {} ", caller_label);
                    let (truncated, _) = truncate_to_width(&label_text, inner_w);
                    surface.put_str(x + 1, y + row, truncated, &default_style);
                }
                surface.put_str(x + 1 + inner_w, y + row, "┐", &default_style);
            } else if row == 1 {
                let prompt = " ";
                let max_input_w = inner_w.saturating_sub(prompt.len());
                let (truncated_input, used_w) = truncate_to_width(&self.input.text, max_input_w);
                let padding = inner_w.saturating_sub(prompt.len() + used_w);

                surface.put_str(x, y + row, "│", &default_style);
                let mut col = x + 1;
                col += surface.put_str(col, y + row, prompt, &default_style);
                col += surface.put_str(col, y + row, truncated_input, &default_style);
                surface.fill_region(col, y + row, padding, ' ', &default_style);
                surface.put_str(x + 1 + inner_w, y + row, "│", &default_style);
            } else if row == 2 && h > 3 {
                surface.put_str(x, y + row, "├", &default_style);
                surface.fill_region(x + 1, y + row, inner_w, '─', &default_style);
                surface.put_str(x + 1 + inner_w, y + row, "┤", &default_style);
            } else if row == h - 1 {
                surface.put_str(x, y + row, "└", &default_style);
                surface.fill_region(x + 1, y + row, inner_w, '─', &default_style);
                surface.put_str(x + 1 + inner_w, y + row, "┘", &default_style);
            } else if candidate_area_h > 0 {
                let candidate_row = row - 3;
                let candidate_idx = self.scroll_offset + candidate_row;

                surface.put_str(x, y + row, "│", &default_style);

                if candidate_idx < self.candidates.len() {
                    let is_selected = candidate_idx == self.selected;

                    let status_color = match self.candidates[candidate_idx].kind {
                        CandidateKind::File(idx) => self
                            .git_status_map
                            .get(&self.file_entries[idx])
                            .map(|s| s.color()),
                        _ => None,
                    };

                    render_candidate_label(
                        surface,
                        x + 1,
                        y + row,
                        &self.candidates[candidate_idx],
                        inner_w,
                        is_selected,
                        status_color,
                    );
                } else {
                    surface.fill_region(x + 1, y + row, inner_w, ' ', &default_style);
                }

                surface.put_str(x + 1 + inner_w, y + row, "│", &default_style);
            } else {
                surface.put_str(x, y + row, "│", &default_style);
                surface.fill_region(x + 1, y + row, inner_w, ' ', &default_style);
                surface.put_str(x + 1 + inner_w, y + row, "│", &default_style);
            }
        }
    }

    fn render_right_panel(
        &self,
        surface: &mut Surface,
        x: usize,
        y: usize,
        w: usize,
        h: usize,
        theme: &Theme,
    ) {
        let inner_w = w.saturating_sub(2);
        let content_h = h.saturating_sub(2);
        let has_preview = !self.preview_lines.is_empty();
        let default_style = CellStyle::default();
        let dim_style = CellStyle {
            dim: true,
            ..CellStyle::default()
        };
        let preview_start_col = if let (Some(target_line), Some(target_char_col)) =
            (self.jump_target_preview_line, self.jump_target_char_col)
        {
            self.preview_lines
                .get(target_line)
                .and_then(|line| jump_marker_column(line, target_char_col))
                .map(|(marker_col, _)| marker_col.saturating_sub(inner_w / 2))
                .unwrap_or(0)
        } else {
            0
        };

        for row in 0..h {
            if row == 0 {
                surface.put_str(x, y + row, "┌", &default_style);
                surface.fill_region(x + 1, y + row, inner_w, '─', &default_style);
                surface.put_str(x + 1 + inner_w, y + row, "┐", &default_style);
            } else if row == h - 1 {
                surface.put_str(x, y + row, "└", &default_style);
                surface.fill_region(x + 1, y + row, inner_w, '─', &default_style);
                surface.put_str(x + 1 + inner_w, y + row, "┘", &default_style);
            } else if has_preview {
                let line_idx = row - 1;
                surface.put_str(x, y + row, "│", &default_style);
                if line_idx < content_h && line_idx < self.preview_lines.len() {
                    let line = &self.preview_lines[line_idx];
                    let window = slice_preview_display_window(line, preview_start_col, inner_w);
                    if let Some(spans) = self.preview_spans.get(&line_idx) {
                        let visible_spans = rebase_preview_spans_to_window(
                            spans,
                            window.start_byte,
                            window.end_byte,
                        );
                        render_highlighted_line(
                            surface,
                            y + row,
                            x + 1,
                            window.visible,
                            &visible_spans,
                            inner_w,
                            theme,
                        );
                    } else {
                        surface.put_str(x + 1, y + row, window.visible, &dim_style);
                        let pad = inner_w.saturating_sub(window.used_width);
                        if pad > 0 {
                            surface.fill_region(
                                x + 1 + window.used_width,
                                y + row,
                                pad,
                                ' ',
                                &default_style,
                            );
                        }
                    }
                    if self.jump_target_preview_line == Some(line_idx)
                        && let Some(target_char_col) = self.jump_target_char_col
                        && let Some((marker_col, marker_width)) =
                            jump_marker_column(line, target_char_col)
                        && marker_col >= window.start_col
                    {
                        let visible_col = marker_col.saturating_sub(window.start_col);
                        let clamped_col = visible_col.min(inner_w.saturating_sub(1));
                        if clamped_col < inner_w {
                            let marker_x = x + 1 + clamped_col;
                            let marker_cell = surface.get_mut(marker_x, y + row);
                            marker_cell.style.reverse = true;

                            if marker_width == 2 && clamped_col + 1 < inner_w {
                                let continuation = surface.get_mut(marker_x + 1, y + row);
                                continuation.style.reverse = true;
                            }
                        }
                    }
                } else {
                    surface.fill_region(x + 1, y + row, inner_w, ' ', &default_style);
                }
                surface.put_str(x + 1 + inner_w, y + row, "│", &default_style);
            } else {
                surface.put_str(x, y + row, "│", &default_style);
                surface.fill_region(x + 1, y + row, inner_w, ' ', &default_style);
                surface.put_str(x + 1 + inner_w, y + row, "│", &default_style);
            }
        }
    }
}

/// Shorten a file path to fit within `max_width` display cells.
/// Always preserves the filename; adds parent directories right-to-left
/// as space permits, prefixed with ".../" when truncated.
/// Returns (display_string, original_chars_skipped, display_prefix_char_count).
fn shorten_path(path: &str, max_width: usize) -> (String, usize, usize) {
    let chars: Vec<char> = path.chars().collect();
    let total_w: usize = chars
        .iter()
        .map(|c| UnicodeWidthChar::width(*c).unwrap_or(0))
        .sum();

    if total_w <= max_width {
        return (path.to_string(), 0, 0);
    }

    let slash_positions: Vec<usize> = chars
        .iter()
        .enumerate()
        .filter_map(|(i, &c)| if c == '/' { Some(i) } else { None })
        .collect();

    if slash_positions.is_empty() {
        let (t, _) = truncate_to_width(path, max_width);
        return (t.to_string(), 0, 0);
    }

    let prefix = ".../";
    let prefix_w = 4;
    let available = max_width.saturating_sub(prefix_w);

    // Try cuts from rightmost '/' to leftmost, finding longest fitting tail
    let mut best: Option<usize> = None;
    for &sp in slash_positions.iter().rev() {
        let tail_start = sp + 1;
        let tail_w: usize = chars[tail_start..]
            .iter()
            .map(|c| UnicodeWidthChar::width(*c).unwrap_or(0))
            .sum();
        if tail_w <= available {
            best = Some(tail_start);
        } else {
            break;
        }
    }

    match best {
        Some(cut) => {
            let tail: String = chars[cut..].iter().collect();
            (format!("{}{}", prefix, tail), cut, prefix.len())
        }
        None => {
            // Even filename doesn't fit with ".../" prefix; show filename only
            let last_slash = *slash_positions.last().unwrap();
            let filename: String = chars[last_slash + 1..].iter().collect();
            let (t, _) = truncate_to_width(&filename, max_width);
            (t.to_string(), last_slash + 1, 0)
        }
    }
}

fn render_candidate_label(
    surface: &mut Surface,
    x: usize,
    y: usize,
    candidate: &ScoredCandidate,
    max_width: usize,
    is_selected: bool,
    status_color: Option<Color>,
) {
    let base_style = if is_selected {
        CellStyle {
            reverse: true,
            fg: status_color,
            ..CellStyle::default()
        }
    } else {
        CellStyle {
            fg: status_color,
            ..CellStyle::default()
        }
    };

    let bold_style = if is_selected {
        CellStyle {
            bold: true,
            reverse: true,
            fg: status_color,
            ..CellStyle::default()
        }
    } else {
        CellStyle {
            bold: true,
            fg: status_color,
            ..CellStyle::default()
        }
    };

    // Write prefix space
    let prefix = " ";
    let mut col = x;
    col += surface.put_str(col, y, prefix, &base_style);

    let effective_w = max_width.saturating_sub(1);

    // For buffer candidates, shorten path to fit; otherwise use label as-is
    let (display_chars, match_set) = if matches!(candidate.kind, CandidateKind::Buffer(_)) {
        let (shortened, skip, prefix_len) = shorten_path(&candidate.label, effective_w);
        let dchars: Vec<char> = shortened.chars().collect();
        let adjusted: HashSet<usize> = candidate
            .match_positions
            .iter()
            .filter(|&&p| p >= skip)
            .map(|&p| p - skip + prefix_len)
            .collect();
        (dchars, adjusted)
    } else {
        let dchars: Vec<char> = candidate.label.chars().collect();
        let set: HashSet<usize> = candidate.match_positions.iter().copied().collect();
        (dchars, set)
    };

    for (i, &ch) in display_chars.iter().enumerate() {
        let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0);
        if col + ch_width > x + max_width {
            break;
        }

        let style = if match_set.contains(&i) {
            &bold_style
        } else {
            &base_style
        };

        let mut buf = [0u8; 4];
        let s = ch.encode_utf8(&mut buf);
        col += surface.put_str(col, y, s, style);
    }

    // Pad remaining
    let used = col - x;
    let padding = max_width.saturating_sub(used);
    if padding > 0 {
        surface.fill_region(col, y, padding, ' ', &base_style);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[test]
    fn fuzzy_match_exact() {
        let result = fuzzy_match("Save File", "Save File");
        assert!(result.is_some());
        let (score, positions) = result.unwrap();
        assert!(score > 0);
        assert_eq!(positions, vec![0, 1, 2, 3, 4, 5, 6, 7, 8]);
    }

    #[test]
    fn fuzzy_match_prefix() {
        let result = fuzzy_match("Save File", "sav");
        assert!(result.is_some());
        let (_, positions) = result.unwrap();
        assert_eq!(positions, vec![0, 1, 2]);
    }

    #[test]
    fn fuzzy_match_case_insensitive() {
        let result = fuzzy_match("Save File", "sf");
        assert!(result.is_some());
        let (_, positions) = result.unwrap();
        assert_eq!(positions[0], 0); // 'S'
        assert_eq!(positions[1], 5); // 'F'
    }

    #[test]
    fn fuzzy_match_no_match() {
        let result = fuzzy_match("Save File", "xyz");
        assert!(result.is_none());
    }

    #[test]
    fn fuzzy_match_empty_needle() {
        let result = fuzzy_match("Save File", "");
        assert!(result.is_some());
        let (score, positions) = result.unwrap();
        assert_eq!(score, 0);
        assert!(positions.is_empty());
    }

    #[test]
    fn fuzzy_match_word_boundary_bonus() {
        let (score_sf, _) = fuzzy_match("Save File", "sf").unwrap();
        let (score_sa, _) = fuzzy_match("Save File", "sa").unwrap();
        assert!(score_sf > 0 || score_sa > 0);
    }

    #[test]
    fn fuzzy_match_consecutive_beats_sparse() {
        let (score_con, _) = fuzzy_match("Save File", "sav").unwrap();
        let (score_spr, _) = fuzzy_match("Save File", "sfe").unwrap();
        assert!(
            score_con > score_spr,
            "consecutive matches should score higher"
        );
    }

    #[test]
    fn fuzzy_match_order_matters() {
        let result = fuzzy_match("Quit Editor", "qe");
        assert!(result.is_some());
        let result2 = fuzzy_match("Quit Editor", "eq");
        assert!(result2.is_none());
    }

    #[test]
    fn fzf_style_match_prefers_consecutive_matches() {
        let (score_consecutive, _) = fzf_style_match("abcdef", "abc").unwrap();
        let (score_sparse, _) = fzf_style_match("axbxcxdef", "abc").unwrap();
        assert!(score_consecutive > score_sparse);
    }

    #[test]
    fn palette_mode_detection() {
        let registry = CommandRegistry::new();
        let lang_registry = LanguageRegistry::new();
        let config = Config::default();
        let mut palette =
            Palette::new(vec![], Path::new(""), &HashMap::new(), None, vec![], vec![]);
        assert_eq!(palette.input.text, ">");
        palette.update_candidates(&registry, &lang_registry, &config);
        assert_eq!(palette.mode, PaletteMode::Command);

        palette.input.text = "hello".into();
        palette.update_candidates(&registry, &lang_registry, &config);
        assert_eq!(palette.mode, PaletteMode::FileFinder);
    }

    #[test]
    fn palette_selection_wraps() {
        let lang_registry = LanguageRegistry::new();
        let config = Config::default();
        let mut palette =
            Palette::new(vec![], Path::new(""), &HashMap::new(), None, vec![], vec![]);
        palette.candidates = vec![
            ScoredCandidate {
                kind: CandidateKind::Command(0),
                label: "A".into(),
                score: 0,
                match_positions: vec![],
                preview_lines: vec![],
            },
            ScoredCandidate {
                kind: CandidateKind::Command(1),
                label: "B".into(),
                score: 0,
                match_positions: vec![],
                preview_lines: vec![],
            },
        ];
        palette.selected = 0;
        palette.select_next(&lang_registry, &config);
        assert_eq!(palette.selected, 1);
        palette.select_next(&lang_registry, &config);
        assert_eq!(palette.selected, 0); // wraps to first
        palette.select_prev(&lang_registry, &config);
        assert_eq!(palette.selected, 1); // wraps to last
        palette.select_prev(&lang_registry, &config);
        assert_eq!(palette.selected, 0);
    }

    #[test]
    fn global_search_literal_case_insensitive() {
        let text = "First Line\nsecond line\nTHIRD line";
        let matches = workers::find_global_search_matches("src/main.rs", text, "line", 10);
        assert_eq!(matches.len(), 3);
        assert_eq!(matches[0].line, 0);
        assert_eq!(matches[1].line, 1);
        assert_eq!(matches[2].line, 2);
        assert!(matches[0].preview_lines[0].starts_with("src/main.rs:1:"));
    }

    #[test]
    fn global_search_enter_opens_selected_match() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        let registry = CommandRegistry::new();
        let lang_registry = LanguageRegistry::new();
        let config = Config::default();
        let mut palette = Palette::new_global_search(vec![], Path::new(""), &HashMap::new());

        palette.global_search_entries = vec![GlobalSearchResultEntry {
            rel_path: "src/main.rs".to_string(),
            line: 12,
            char_col: 7,
            preview_lines: vec![
                "src/main.rs:13:8".to_string(),
                "   13 | let x = 1;".to_string(),
            ],
        }];
        palette.candidates = vec![ScoredCandidate {
            kind: CandidateKind::SearchResult(0),
            label: "src/main.rs:13 let x = 1;".to_string(),
            score: 0,
            match_positions: vec![],
            preview_lines: vec![
                "src/main.rs:13:8".to_string(),
                "   13 | let x = 1;".to_string(),
            ],
        }];

        let result = palette.handle_key_event(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &registry,
            &lang_registry,
            &config,
        );

        assert_eq!(
            result,
            EventResult::Action(Action::App(AppAction::Buffer(
                BufferAction::OpenProjectFileAt {
                    rel_path: "src/main.rs".to_string(),
                    line: 12,
                    char_col: 7,
                },
            )))
        );
    }

    #[test]
    fn global_search_worker_error_updates_preview_message() {
        let mut palette = Palette::new_global_search(vec![], Path::new(""), &HashMap::new());
        let (tx, rx) = mpsc::channel::<GlobalSearchBatch>();
        palette.global_search_result_rx = Some(rx);

        tx.send(GlobalSearchBatch {
            generation: 1,
            results: Vec::new(),
            error: Some("bad global search filter".to_string()),
        })
        .unwrap();

        palette.pump_global_search();

        assert!(palette.global_search_entries.is_empty());
        assert!(palette.candidates.is_empty());
        assert_eq!(
            palette.preview_lines,
            vec!["Global search error: bad global search filter".to_string()]
        );
    }

    #[test]
    fn buffer_picker_mode() {
        let entries = vec![
            (1, "main.rs".to_string(), vec!["fn main() {}".to_string()]),
            (2, "[scratch]".to_string(), vec![]),
            (3, "lib.rs".to_string(), vec!["pub mod foo;".to_string()]),
        ];
        let palette = Palette::new_buffer_picker(entries);
        assert_eq!(palette.mode, PaletteMode::BufferPicker);
        assert_eq!(palette.candidates.len(), 3);
        assert_eq!(palette.selected_buffer_id(), Some(1));
    }

    #[test]
    fn buffer_picker_filter() {
        let entries = vec![
            (1, "main.rs".to_string(), vec!["fn main() {}".to_string()]),
            (2, "[scratch]".to_string(), vec![]),
            (3, "lib.rs".to_string(), vec!["pub mod foo;".to_string()]),
        ];
        let mut palette = Palette::new_buffer_picker(entries);
        palette.on_char_buffer('m');
        // "main.rs" matches, "[scratch]" doesn't have 'm'. "lib.rs" doesn't either.
        assert!(!palette.candidates.is_empty());
        assert_eq!(palette.selected_buffer_id(), Some(1));
    }

    #[test]
    fn buffer_picker_preview_populated_on_creation() {
        let entries = vec![
            (
                1,
                "main.rs".to_string(),
                vec!["fn main() {}".to_string(), "// end".to_string()],
            ),
            (2, "[scratch]".to_string(), vec![]),
        ];
        let palette = Palette::new_buffer_picker(entries);
        assert_eq!(palette.preview_lines, vec!["fn main() {}", "// end"]);
    }

    #[test]
    fn buffer_picker_preview_updates_on_selection_change() {
        let entries = vec![
            (1, "main.rs".to_string(), vec!["fn main() {}".to_string()]),
            (2, "lib.rs".to_string(), vec!["pub mod foo;".to_string()]),
        ];
        let lang_registry = LanguageRegistry::new();
        let config = Config::default();
        let mut palette = Palette::new_buffer_picker(entries);
        assert_eq!(palette.preview_lines, vec!["fn main() {}"]);
        palette.select_next(&lang_registry, &config);
        assert_eq!(palette.preview_lines, vec!["pub mod foo;"]);
    }

    #[test]
    fn buffer_picker_preview_updates_on_filter() {
        let entries = vec![
            (1, "main.rs".to_string(), vec!["fn main() {}".to_string()]),
            (2, "lib.rs".to_string(), vec!["pub mod foo;".to_string()]),
        ];
        let mut palette = Palette::new_buffer_picker(entries);
        palette.on_char_buffer('l');
        // only "lib.rs" matches
        assert_eq!(palette.candidates.len(), 1);
        assert_eq!(palette.preview_lines, vec!["pub mod foo;"]);
    }

    #[test]
    fn jump_picker_mode_and_selection() {
        let entries = vec![
            JumpPickerEntry {
                jump_index: 3,
                label: "src/main.rs:10:1 main".to_string(),
                preview_lines: vec![
                    "src/main.rs:10:1".to_string(),
                    "   10 | fn main() {}".to_string(),
                ],
                source_path: Some("src/main.rs".to_string()),
                target_preview_line: Some(1),
                target_char_col: 3,
            },
            JumpPickerEntry {
                jump_index: 1,
                label: "src/lib.rs:2:1 run".to_string(),
                preview_lines: vec![
                    "src/lib.rs:2:1".to_string(),
                    "    2 | pub fn run() {}".to_string(),
                ],
                source_path: Some("src/lib.rs".to_string()),
                target_preview_line: Some(1),
                target_char_col: 8,
            },
        ];
        let palette = Palette::new_jump_picker(entries);
        assert_eq!(palette.mode, PaletteMode::JumpPicker);
        assert_eq!(palette.selected_jump_index(), Some(3));
        assert_eq!(
            palette.preview_lines,
            vec!["src/main.rs:10:1", "   10 | fn main() {}"]
        );
    }

    #[test]
    fn jump_picker_enter_dispatches_jump_action() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        let registry = CommandRegistry::new();
        let lang_registry = LanguageRegistry::new();
        let config = Config::default();
        let entries = vec![JumpPickerEntry {
            jump_index: 7,
            label: "src/main.rs:1:1 main".to_string(),
            preview_lines: vec!["src/main.rs:1:1".to_string()],
            source_path: Some("src/main.rs".to_string()),
            target_preview_line: None,
            target_char_col: 0,
        }];
        let mut palette = Palette::new_jump_picker(entries);

        let result = palette.handle_key_event(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &registry,
            &lang_registry,
            &config,
        );
        assert_eq!(
            result,
            EventResult::Action(Action::App(AppAction::Navigation(
                NavigationAction::JumpToListIndex(7),
            )))
        );
    }

    #[test]
    fn jump_picker_filter_matches_word_suffix_with_fzf() {
        let entries = vec![
            JumpPickerEntry {
                jump_index: 3,
                label: "src/main.rs:10:1 helper".to_string(),
                preview_lines: vec!["src/main.rs:10:1".to_string()],
                source_path: Some("src/main.rs".to_string()),
                target_preview_line: None,
                target_char_col: 0,
            },
            JumpPickerEntry {
                jump_index: 1,
                label: "src/lib.rs:2:1 render_overlay".to_string(),
                preview_lines: vec!["src/lib.rs:2:1".to_string()],
                source_path: Some("src/lib.rs".to_string()),
                target_preview_line: None,
                target_char_col: 0,
            },
        ];
        let mut palette = Palette::new_jump_picker(entries);
        palette.on_char_jump('o');
        palette.on_char_jump('v');
        palette.on_char_jump('r');
        assert_eq!(palette.candidates.len(), 1);
        assert_eq!(palette.candidates[0].label, "src/lib.rs:2:1 render_overlay");
    }

    #[test]
    fn jump_picker_preview_builds_syntax_spans() {
        let entries = vec![JumpPickerEntry {
            jump_index: 1,
            label: "src/main.rs:1:1 main".to_string(),
            preview_lines: vec![
                "src/main.rs:1:1".to_string(),
                "    1 | fn main() {}".to_string(),
            ],
            source_path: Some("src/main.rs".to_string()),
            target_preview_line: Some(1),
            target_char_col: 3,
        }];
        let palette = Palette::new_jump_picker(entries);
        assert!(!palette.preview_spans.is_empty());
    }

    fn right_preview_panel_geometry(cols: usize, rows: usize) -> (usize, usize, usize) {
        let popup_w = (cols * 80 / 100).max(3);
        let popup_h = (rows * 80 / 100).max(3);
        let offset_x = (cols.saturating_sub(popup_w)) / 2;
        let _offset_y = (rows.saturating_sub(popup_h)) / 2;
        let gap = 2;
        let left_w = (popup_w - gap) / 2;
        let right_w = popup_w - gap - left_w;
        let right_x = offset_x + left_w + gap;
        (right_x + 1, right_w.saturating_sub(2), popup_h)
    }

    fn reversed_columns_on_row(surface: &Surface, row: usize) -> Vec<usize> {
        (0..surface.width)
            .filter(|&x| surface.get(x, row).style.reverse)
            .collect()
    }

    #[test]
    fn jump_picker_preview_auto_scrolls_to_center_marker() {
        use crate::syntax::theme::Theme;
        use crate::ui::framework::surface::Surface;

        let long = format!("let left = 1; let right = {}; // marker", "x".repeat(100));
        let mut palette = Palette::new_jump_picker(vec![JumpPickerEntry {
            jump_index: 1,
            label: "src/main.rs:1:1 main".to_string(),
            preview_lines: vec!["src/main.rs:1:1".to_string(), format!("    1 | {}", long)],
            source_path: Some("src/main.rs".to_string()),
            target_preview_line: Some(1),
            target_char_col: 85,
        }]);

        let mut surface = Surface::new(100, 20);
        let theme = Theme::dark();
        let _ = palette.render_overlay(&mut surface, &theme);
        let (preview_x, inner_w, _) = right_preview_panel_geometry(100, 20);
        let reversed = reversed_columns_on_row(&surface, 4);
        assert!(!reversed.is_empty());
        let center = preview_x + inner_w / 2;
        assert!((reversed[0] as isize - center as isize).abs() <= 2);
    }

    #[test]
    fn jump_picker_preview_keeps_syntax_when_horizontally_sliced() {
        use crate::syntax::theme::Theme;
        use crate::ui::framework::surface::Surface;

        let mut palette = Palette::new_jump_picker(vec![JumpPickerEntry {
            jump_index: 1,
            label: "src/main.rs:1:1 main".to_string(),
            preview_lines: vec![
                "src/main.rs:1:1".to_string(),
                format!(
                    "    1 | fn main() {{ let target = \"{}\"; }}",
                    "value".repeat(20)
                ),
            ],
            source_path: Some("src/main.rs".to_string()),
            target_preview_line: Some(1),
            target_char_col: 70,
        }]);

        let mut surface = Surface::new(100, 20);
        let theme = Theme::dark();
        let _ = palette.render_overlay(&mut surface, &theme);
        let styled_cells = (0..surface.width)
            .filter(|&x| surface.get(x, 4).style.fg.is_some())
            .count();
        assert!(styled_cells > 0);
    }

    #[test]
    fn symbol_picker_preview_auto_scrolls_to_center_marker() {
        use crate::syntax::theme::Theme;
        use crate::ui::framework::surface::Surface;

        let long = format!("let left = 1; let right = {}; // marker", "x".repeat(100));
        let mut palette = Palette::new_symbol_picker(vec![(
            "marker [function]  10:86".to_string(),
            9,
            85,
            vec![format!("   10 | {}", long)],
        )]);

        let mut surface = Surface::new(100, 20);
        let theme = Theme::dark();
        let _ = palette.render_overlay(&mut surface, &theme);
        let (preview_x, inner_w, _) = right_preview_panel_geometry(100, 20);
        let reversed = reversed_columns_on_row(&surface, 3);
        assert!(!reversed.is_empty());
        let center = preview_x + inner_w / 2;
        assert!((reversed[0] as isize - center as isize).abs() <= 2);
    }

    #[test]
    fn jump_picker_preview_marks_wide_char_continuation() {
        use crate::syntax::theme::Theme;
        use crate::ui::framework::surface::Surface;

        let code = format!("let s = \"{}あ{}\";", "x".repeat(40), "x".repeat(40));
        let target_char_col = code
            .chars()
            .position(|ch| ch == 'あ')
            .expect("contains wide char");
        let mut palette = Palette::new_jump_picker(vec![JumpPickerEntry {
            jump_index: 1,
            label: "src/main.rs:1:1 wide".to_string(),
            preview_lines: vec!["src/main.rs:1:1".to_string(), format!("    1 | {}", code)],
            source_path: Some("src/main.rs".to_string()),
            target_preview_line: Some(1),
            target_char_col,
        }]);

        let mut surface = Surface::new(100, 20);
        let theme = Theme::dark();
        let _ = palette.render_overlay(&mut surface, &theme);
        let reversed = reversed_columns_on_row(&surface, 4);
        assert!(reversed.len() >= 2);
    }

    #[test]
    fn jump_picker_preview_defaults_to_no_horizontal_scroll_without_target() {
        use crate::syntax::theme::Theme;
        use crate::ui::framework::surface::Surface;

        let mut palette = Palette::new_jump_picker(vec![JumpPickerEntry {
            jump_index: 1,
            label: "src/main.rs:1:1".to_string(),
            preview_lines: vec!["src/main.rs:1:1".to_string()],
            source_path: Some("src/main.rs".to_string()),
            target_preview_line: None,
            target_char_col: 0,
        }]);

        let mut surface = Surface::new(100, 20);
        let theme = Theme::dark();
        let _ = palette.render_overlay(&mut surface, &theme);
        let (preview_x, _, _) = right_preview_panel_geometry(100, 20);
        assert_eq!(surface.get(preview_x, 3).symbol, "s");
    }

    #[test]
    fn reference_picker_mode_and_enter_dispatches_open_file_at_lsp_location() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        let registry = CommandRegistry::new();
        let lang_registry = LanguageRegistry::new();
        let config = Config::default();
        let target = PathBuf::from("/tmp/src/main.rs");
        let mut palette = Palette::new_reference_picker(
            "LSP: Find References".to_string(),
            vec![ReferencePickerEntry {
                label: "src/main.rs:10:4 helper".to_string(),
                path: target.clone(),
                line: 9,
                character_utf16: 3,
                preview_lines: vec![
                    "src/main.rs:10:4".to_string(),
                    "   10 | fn helper() {}".to_string(),
                ],
                source_path: Some("src/main.rs".to_string()),
                target_preview_line: Some(1),
                target_char_col: 3,
            }],
        );
        assert_eq!(palette.mode, PaletteMode::ReferencePicker);

        let result = palette.handle_key_event(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &registry,
            &lang_registry,
            &config,
        );
        assert_eq!(
            result,
            EventResult::Action(Action::App(AppAction::Navigation(
                NavigationAction::OpenFileAtLspLocation {
                    path: target,
                    line: 9,
                    character_utf16: 3,
                },
            )))
        );
    }

    #[test]
    fn git_branch_picker_mode_and_enter_dispatches_switch_branch() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        let registry = CommandRegistry::new();
        let lang_registry = LanguageRegistry::new();
        let config = Config::default();
        let mut palette = Palette::new_git_branch_picker(vec![
            GitBranchPickerEntry {
                branch_name: "feature/login".to_string(),
                label: "  feature/login".to_string(),
                preview_lines: vec!["Branch: feature/login".to_string()],
            },
            GitBranchPickerEntry {
                branch_name: "main".to_string(),
                label: "* main".to_string(),
                preview_lines: vec!["Branch: main".to_string()],
            },
        ]);

        assert_eq!(palette.mode, PaletteMode::GitBranchPicker);
        assert_eq!(palette.selected_git_branch().as_deref(), Some("main"));

        let result = palette.handle_key_event(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &registry,
            &lang_registry,
            &config,
        );
        assert_eq!(
            result,
            EventResult::Action(Action::App(AppAction::Project(
                ProjectAction::SwitchGitBranch("main".to_string()),
            )))
        );
    }

    #[test]
    fn reference_picker_filter_matches_with_fzf() {
        let mut palette = Palette::new_reference_picker(
            "LSP: Find References".to_string(),
            vec![
                ReferencePickerEntry {
                    label: "src/main.rs:10:4 helper".to_string(),
                    path: PathBuf::from("/tmp/src/main.rs"),
                    line: 9,
                    character_utf16: 3,
                    preview_lines: vec!["src/main.rs:10:4".to_string()],
                    source_path: Some("src/main.rs".to_string()),
                    target_preview_line: None,
                    target_char_col: 3,
                },
                ReferencePickerEntry {
                    label: "src/lib.rs:2:1 render_overlay".to_string(),
                    path: PathBuf::from("/tmp/src/lib.rs"),
                    line: 1,
                    character_utf16: 0,
                    preview_lines: vec!["src/lib.rs:2:1".to_string()],
                    source_path: Some("src/lib.rs".to_string()),
                    target_preview_line: None,
                    target_char_col: 0,
                },
            ],
        );

        let registry = CommandRegistry::new();
        let lang_registry = LanguageRegistry::new();
        let config = Config::default();
        palette.on_char('o', &registry, &lang_registry, &config);
        palette.on_char('v', &registry, &lang_registry, &config);
        palette.on_char('r', &registry, &lang_registry, &config);

        assert_eq!(palette.candidates.len(), 1);
        assert_eq!(palette.candidates[0].label, "src/lib.rs:2:1 render_overlay");
    }

    #[test]
    fn reference_picker_reuses_cached_preview_spans_on_revisit() {
        let lang_registry = LanguageRegistry::new();
        let config = Config::default();
        let mut palette = Palette::new_reference_picker(
            "LSP: Find References".to_string(),
            vec![
                ReferencePickerEntry {
                    label: "src/main.rs:10:4 helper".to_string(),
                    path: PathBuf::from("/tmp/src/main.rs"),
                    line: 9,
                    character_utf16: 3,
                    preview_lines: vec![
                        "src/main.rs:10:4".to_string(),
                        "   10 | fn helper() {}".to_string(),
                    ],
                    source_path: Some("src/main.rs".to_string()),
                    target_preview_line: Some(1),
                    target_char_col: 3,
                },
                ReferencePickerEntry {
                    label: "src/lib.rs:2:1 render_overlay".to_string(),
                    path: PathBuf::from("/tmp/src/lib.rs"),
                    line: 1,
                    character_utf16: 0,
                    preview_lines: vec![
                        "src/lib.rs:2:1".to_string(),
                        "    2 | pub fn render_overlay() {}".to_string(),
                    ],
                    source_path: Some("src/lib.rs".to_string()),
                    target_preview_line: Some(1),
                    target_char_col: 7,
                },
            ],
        );

        assert!(palette.reference_highlight_cache.contains_key(&0));
        palette.select_next(&lang_registry, &config);
        assert!(palette.reference_highlight_cache.contains_key(&1));

        let mut sentinel = HashMap::new();
        sentinel.insert(
            1,
            vec![HighlightSpan {
                start: 0,
                end: 1,
                capture_name: "sentinel.capture".to_string(),
            }],
        );
        palette.reference_highlight_cache.insert(0, sentinel);

        palette.select_prev(&lang_registry, &config);
        let spans = palette
            .preview_spans
            .get(&1)
            .expect("cached spans should be used");
        assert_eq!(spans[0].capture_name, "sentinel.capture");
    }

    #[test]
    fn reference_picker_renders_caller_label_on_top_border() {
        use crate::syntax::theme::Theme;
        use crate::ui::framework::surface::Surface;

        let mut palette = Palette::new_reference_picker(
            "LSP: Find References".to_string(),
            vec![ReferencePickerEntry {
                label: "src/main.rs:1:1".to_string(),
                path: PathBuf::from("/tmp/src/main.rs"),
                line: 0,
                character_utf16: 0,
                preview_lines: vec!["src/main.rs:1:1".to_string()],
                source_path: Some("src/main.rs".to_string()),
                target_preview_line: None,
                target_char_col: 0,
            }],
        );

        let mut surface = Surface::new(100, 20);
        let theme = Theme::dark();
        let _ = palette.render_overlay(&mut surface, &theme);

        let popup_w = 100 * 80 / 100;
        let popup_h = 20 * 80 / 100;
        let offset_x = (100usize.saturating_sub(popup_w)) / 2;
        let offset_y = (20usize.saturating_sub(popup_h)) / 2;
        let left_w = (popup_w - 2) / 2;

        let row_text: String = (offset_x + 1..offset_x + 1 + left_w.saturating_sub(2))
            .map(|x| {
                surface
                    .get(x, offset_y)
                    .symbol
                    .chars()
                    .next()
                    .unwrap_or(' ')
            })
            .collect();
        assert!(row_text.contains("LSP: Find References"));
    }

    #[test]
    fn symbol_picker_mode_and_enter_dispatches_jump_line_char() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        let registry = CommandRegistry::new();
        let lang_registry = LanguageRegistry::new();
        let config = Config::default();
        let mut palette = Palette::new_symbol_picker(vec![(
            "helper [function]  10:4".to_string(),
            9,
            3,
            vec!["   10 | fn helper() {}".to_string()],
        )]);
        assert_eq!(palette.mode, PaletteMode::SymbolPicker);

        let result = palette.handle_key_event(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &registry,
            &lang_registry,
            &config,
        );
        assert_eq!(
            result,
            EventResult::Action(Action::App(AppAction::Navigation(
                NavigationAction::JumpToLineChar {
                    line: 9,
                    char_col: 3,
                },
            )))
        );
    }

    #[test]
    fn smart_copy_picker_enter_dispatches_copy_to_clipboard() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        let registry = CommandRegistry::new();
        let lang_registry = LanguageRegistry::new();
        let config = Config::default();
        let mut palette = Palette::new_smart_copy_picker(vec![SmartCopyPickerEntry {
            label: "helper [function]  10:4".to_string(),
            line: 9,
            char_col: 3,
            preview_lines: vec!["   10 | fn helper() {}".to_string()],
            copy_text: "fn helper() {}".to_string(),
        }]);
        assert_eq!(palette.mode, PaletteMode::SymbolPicker);

        let result = palette.handle_key_event(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &registry,
            &lang_registry,
            &config,
        );
        assert_eq!(
            result,
            EventResult::Action(Action::App(AppAction::Integration(
                IntegrationAction::CopyToClipboard {
                    text: "fn helper() {}".to_string(),
                    description: "smart copy section".to_string(),
                },
            )))
        );
    }

    #[test]
    fn picker_ctrl_f_b_moves_input_cursor() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        let registry = CommandRegistry::new();
        let lang_registry = LanguageRegistry::new();
        let config = Config::default();
        let mut palette =
            Palette::new(vec![], Path::new(""), &HashMap::new(), None, vec![], vec![]);
        palette.set_input("abc".to_string());
        palette.input.cursor = 1;

        palette.handle_key_event(
            KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL),
            &registry,
            &lang_registry,
            &config,
        );
        assert_eq!(palette.input.cursor, 2);

        palette.handle_key_event(
            KeyEvent::new(KeyCode::Char('b'), KeyModifiers::CONTROL),
            &registry,
            &lang_registry,
            &config,
        );
        assert_eq!(palette.input.cursor, 1);
    }

    #[test]
    fn picker_ctrl_w_and_ctrl_k_edit_query() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        let registry = CommandRegistry::new();
        let lang_registry = LanguageRegistry::new();
        let config = Config::default();
        let mut palette =
            Palette::new(vec![], Path::new(""), &HashMap::new(), None, vec![], vec![]);
        palette.set_input("alpha beta gamma".to_string());
        palette.input.cursor = palette.input.char_len();

        palette.handle_key_event(
            KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL),
            &registry,
            &lang_registry,
            &config,
        );
        assert_eq!(palette.input.text, "alpha beta ");

        palette.input.cursor = 6;
        palette.handle_key_event(
            KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL),
            &registry,
            &lang_registry,
            &config,
        );
        assert_eq!(palette.input.text, "alpha ");
    }

    #[test]
    fn picker_ctrl_j_selects_next_candidate() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        let registry = CommandRegistry::new();
        let lang_registry = LanguageRegistry::new();
        let config = Config::default();
        let mut palette =
            Palette::new(vec![], Path::new(""), &HashMap::new(), None, vec![], vec![]);
        palette.candidates = vec![
            ScoredCandidate {
                kind: CandidateKind::Command(0),
                label: "A".into(),
                score: 0,
                match_positions: vec![],
                preview_lines: vec![],
            },
            ScoredCandidate {
                kind: CandidateKind::Command(1),
                label: "B".into(),
                score: 0,
                match_positions: vec![],
                preview_lines: vec![],
            },
        ];
        palette.selected = 0;

        palette.handle_key_event(
            KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL),
            &registry,
            &lang_registry,
            &config,
        );
        assert_eq!(palette.selected, 1);
    }

    #[test]
    fn ctrl_c_closes_palette() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let registry = CommandRegistry::new();
        let lang_registry = LanguageRegistry::new();
        let config = Config::default();
        let mut palette =
            Palette::new(vec![], Path::new(""), &HashMap::new(), None, vec![], vec![]);
        let key = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        let result = palette.handle_key_event(key, &registry, &lang_registry, &config);
        assert_eq!(
            result,
            EventResult::Action(Action::Ui(UiAction::ClosePalette))
        );
    }

    #[test]
    fn ctrl_q_closes_palette() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let registry = CommandRegistry::new();
        let lang_registry = LanguageRegistry::new();
        let config = Config::default();
        let mut palette =
            Palette::new(vec![], Path::new(""), &HashMap::new(), None, vec![], vec![]);
        let key = KeyEvent::new(KeyCode::Char('q'), KeyModifiers::CONTROL);
        let result = palette.handle_key_event(key, &registry, &lang_registry, &config);
        assert_eq!(
            result,
            EventResult::Action(Action::Ui(UiAction::ClosePalette))
        );
    }

    #[test]
    fn shorten_path_fits() {
        let (display, skip, prefix_len) = shorten_path("src/main.rs", 20);
        assert_eq!(display, "src/main.rs");
        assert_eq!(skip, 0);
        assert_eq!(prefix_len, 0);
    }

    #[test]
    fn shorten_path_truncates_leading_dirs() {
        // "src/deeply/nested/dir/file.rs" = 28 chars
        // With max_width=19: ".../dir/file.rs" = 15, fits
        let (display, skip, prefix_len) = shorten_path("src/deeply/nested/dir/file.rs", 19);
        assert_eq!(display, ".../dir/file.rs");
        assert!(skip > 0);
        assert_eq!(prefix_len, 4);
    }

    #[test]
    fn shorten_path_shows_filename_only() {
        // max_width=12: ".../file.rs" = 11, fits
        let (display, _, _) = shorten_path("src/deeply/nested/dir/file.rs", 12);
        assert_eq!(display, ".../file.rs");
    }

    #[test]
    fn shorten_path_no_slash() {
        // No directory separators, truncate from right
        let (display, skip, prefix_len) = shorten_path("verylongfilename.rs", 10);
        assert_eq!(skip, 0);
        assert_eq!(prefix_len, 0);
        assert!(display.len() <= 10);
    }

    #[test]
    fn shorten_path_scratch_fits() {
        let (display, skip, prefix_len) = shorten_path("[scratch]", 20);
        assert_eq!(display, "[scratch]");
        assert_eq!(skip, 0);
        assert_eq!(prefix_len, 0);
    }

    #[test]
    fn command_history_sorts_by_last_used() {
        use crate::command::history::CommandHistory;
        use crate::command::registry::{CommandEntry, CommandRegistry};
        use std::time::SystemTime;

        // Create unique temp dir for this test
        let timestamp = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let temp_dir =
            std::env::temp_dir().join(format!("gargo_test_palette_history_{}", timestamp));
        std::fs::create_dir_all(&temp_dir).unwrap();

        // Create a test history and record some commands
        let history = CommandHistory::new_with_data_dir(
            &std::path::PathBuf::from("/tmp/test_repo_palette"),
            temp_dir.clone(),
        );

        // Record commands in specific order: Save, Quit, Open
        history.record_execution("test.save").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
        history.record_execution("test.quit").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
        history.record_execution("test.open").unwrap();

        // Create a registry with test commands
        let mut registry = CommandRegistry::new();
        registry.register(CommandEntry {
            id: "test.save".into(),
            label: "Save File".into(),
            category: None,
            action: Box::new(|_ctx| crate::command::registry::CommandEffect::None),
        });
        registry.register(CommandEntry {
            id: "test.open".into(),
            label: "Open File".into(),
            category: None,
            action: Box::new(|_ctx| crate::command::registry::CommandEffect::None),
        });
        registry.register(CommandEntry {
            id: "test.quit".into(),
            label: "Quit Editor".into(),
            category: None,
            action: Box::new(|_ctx| crate::command::registry::CommandEffect::None),
        });
        registry.register(CommandEntry {
            id: "test.unused".into(),
            label: "Unused Command".into(),
            category: None,
            action: Box::new(|_ctx| crate::command::registry::CommandEffect::None),
        });

        // Test: Empty query WITH history should sort by last-used
        let config = Config::default();
        let candidates = Palette::filter_commands(&registry, "", Some(&history), &config);

        // Most recent first: Open, Quit, Save, then alphabetically: Unused
        assert_eq!(candidates.len(), 4);
        assert_eq!(candidates[0].label, "Open File"); // Most recent
        assert_eq!(candidates[1].label, "Quit Editor"); // Second most recent
        assert_eq!(candidates[2].label, "Save File"); // Third most recent
        assert_eq!(candidates[3].label, "Unused Command"); // Not in history, alphabetical

        // Clean up
        std::fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn command_history_alphabetical_without_history() {
        use crate::command::registry::{CommandEntry, CommandRegistry};

        // Create a registry with test commands
        let mut registry = CommandRegistry::new();
        registry.register(CommandEntry {
            id: "test.zzz".into(),
            label: "Zzz Last".into(),
            category: None,
            action: Box::new(|_ctx| crate::command::registry::CommandEffect::None),
        });
        registry.register(CommandEntry {
            id: "test.aaa".into(),
            label: "Aaa First".into(),
            category: None,
            action: Box::new(|_ctx| crate::command::registry::CommandEffect::None),
        });
        registry.register(CommandEntry {
            id: "test.mmm".into(),
            label: "Mmm Middle".into(),
            category: None,
            action: Box::new(|_ctx| crate::command::registry::CommandEffect::None),
        });

        // Test: Empty query WITHOUT history should sort alphabetically
        let config = Config::default();
        let candidates = Palette::filter_commands(&registry, "", None, &config);

        assert_eq!(candidates.len(), 3);
        assert_eq!(candidates[0].label, "Aaa First");
        assert_eq!(candidates[1].label, "Mmm Middle");
        assert_eq!(candidates[2].label, "Zzz Last");
    }

    #[test]
    fn command_history_fuzzy_search_overrides_history() {
        use crate::command::history::CommandHistory;
        use crate::command::registry::{CommandEntry, CommandRegistry};
        use std::time::SystemTime;

        // Create unique temp dir for this test
        let timestamp = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let temp_dir = std::env::temp_dir().join(format!("gargo_test_fuzzy_history_{}", timestamp));
        std::fs::create_dir_all(&temp_dir).unwrap();

        // Create a test history
        let history = CommandHistory::new_with_data_dir(
            &std::path::PathBuf::from("/tmp/test_repo_fuzzy"),
            temp_dir.clone(),
        );

        // Record "Quit" as most recent
        history.record_execution("test.quit").unwrap();

        // Create a registry with test commands
        let mut registry = CommandRegistry::new();
        registry.register(CommandEntry {
            id: "test.save".into(),
            label: "Save File".into(),
            category: None,
            action: Box::new(|_ctx| crate::command::registry::CommandEffect::None),
        });
        registry.register(CommandEntry {
            id: "test.quit".into(),
            label: "Quit Editor".into(),
            category: None,
            action: Box::new(|_ctx| crate::command::registry::CommandEffect::None),
        });

        // Test: Query "save" should match by fuzzy score, not history
        let config = Config::default();
        let candidates = Palette::filter_commands(&registry, "save", Some(&history), &config);

        // Only "Save File" should match
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].label, "Save File");

        // Clean up
        std::fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn command_history_graceful_degradation() {
        use crate::command::registry::{CommandEntry, CommandRegistry};

        // Create a registry with test commands
        let mut registry = CommandRegistry::new();
        registry.register(CommandEntry {
            id: "test.cmd".into(),
            label: "Test Command".into(),
            category: None,
            action: Box::new(|_ctx| crate::command::registry::CommandEffect::None),
        });

        // Test with None history (should not crash, just use alphabetical)
        let config = Config::default();
        let candidates = Palette::filter_commands(&registry, "", None, &config);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].label, "Test Command");
    }

    #[test]
    fn command_labels_for_config_toggles_are_dynamic() {
        let mut registry = CommandRegistry::new();
        crate::command::registry::register_builtins(&mut registry);

        let config = Config {
            debug: false,
            show_line_number: true,
            ..Config::default()
        };
        let candidates = Palette::filter_commands(&registry, "", None, &config);
        let labels: Vec<&str> = candidates.iter().map(|c| c.label.as_str()).collect();
        assert!(labels.contains(&"Show Debug"));
        assert!(labels.contains(&"Hide Line Number"));

        let config = Config {
            debug: true,
            show_line_number: false,
            ..Config::default()
        };
        let candidates = Palette::filter_commands(&registry, "", None, &config);
        let labels: Vec<&str> = candidates.iter().map(|c| c.label.as_str()).collect();
        assert!(labels.contains(&"Hide Debug"));
        assert!(labels.contains(&"Show Line Number"));
    }

    #[test]
    fn release_key_events_are_ignored() {
        use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

        let registry = CommandRegistry::new();
        let lang_registry = LanguageRegistry::new();
        let config = Config::default();
        let mut palette = Palette::new_global_search(vec![], Path::new(""), &HashMap::new());
        palette.input.text = "test".to_string();

        // Simulate Enter key release (happens during IME composition confirmation)
        let result = palette.handle_key_event(
            KeyEvent::new_with_kind(KeyCode::Enter, KeyModifiers::NONE, KeyEventKind::Release),
            &registry,
            &lang_registry,
            &config,
        );

        // Release events should be consumed without action, input preserved
        assert_eq!(result, EventResult::Consumed);
        assert_eq!(palette.input.text, "test"); // Input should NOT be cleared
    }

    #[test]
    fn palette_insert_text_japanese() {
        // Test that Japanese text (from IME paste events) is correctly inserted
        let registry = CommandRegistry::new();
        let lang_registry = LanguageRegistry::new();
        let config = Config::default();
        let mut palette = Palette::new_global_search(vec![], Path::new(""), &HashMap::new());

        // Insert Japanese text (simulating IME composition result)
        palette.insert_text("日本語", &registry, &lang_registry, &config);
        assert_eq!(palette.input.text, "日本語");
        assert_eq!(palette.input.cursor, 3); // 3 characters

        // Insert more Japanese text
        palette.insert_text("テスト", &registry, &lang_registry, &config);
        assert_eq!(palette.input.text, "日本語テスト");
        assert_eq!(palette.input.cursor, 6);
    }

    #[test]
    fn palette_at_prefix_activates_symbol_mode() {
        let registry = CommandRegistry::new();
        let lang_registry = LanguageRegistry::new();
        let config = Config::default();
        let symbols = vec![(
            "main [function]  1:1".to_string(),
            0,
            0,
            vec!["    1 | fn main() {}".to_string()],
        )];
        let mut palette = Palette::new(
            vec![],
            Path::new(""),
            &HashMap::new(),
            None,
            symbols,
            vec![],
        );
        palette.set_input("@".to_string());
        palette.update_candidates(&registry, &lang_registry, &config);
        assert_eq!(palette.mode, PaletteMode::SymbolPicker);
        assert_eq!(palette.candidates.len(), 1);
    }

    #[test]
    fn palette_colon_prefix_activates_goto_line_mode() {
        let registry = CommandRegistry::new();
        let lang_registry = LanguageRegistry::new();
        let config = Config::default();
        let doc_lines = vec![
            "line one".to_string(),
            "line two".to_string(),
            "line three".to_string(),
        ];
        let mut palette = Palette::new(
            vec![],
            Path::new(""),
            &HashMap::new(),
            None,
            vec![],
            doc_lines,
        );
        palette.set_input(":2".to_string());
        palette.update_candidates(&registry, &lang_registry, &config);
        assert_eq!(palette.mode, PaletteMode::GotoLine);
        assert!(palette.candidates.is_empty());
    }

    #[test]
    fn palette_mode_transition_on_prefix_change() {
        let registry = CommandRegistry::new();
        let lang_registry = LanguageRegistry::new();
        let config = Config::default();
        let symbols = vec![("main [function]  1:1".to_string(), 0, 0, vec![])];
        let files = vec!["src/main.rs".to_string()];
        let mut palette =
            Palette::new(files, Path::new(""), &HashMap::new(), None, symbols, vec![]);

        // Start in file mode
        palette.set_input(String::new());
        palette.update_candidates(&registry, &lang_registry, &config);
        assert_eq!(palette.mode, PaletteMode::FileFinder);

        // Switch to symbol mode
        palette.set_input("@".to_string());
        palette.update_candidates(&registry, &lang_registry, &config);
        assert_eq!(palette.mode, PaletteMode::SymbolPicker);

        // Switch to command mode
        palette.set_input(">".to_string());
        palette.update_candidates(&registry, &lang_registry, &config);
        assert_eq!(palette.mode, PaletteMode::Command);

        // Switch to goto line mode
        palette.set_input(":".to_string());
        palette.update_candidates(&registry, &lang_registry, &config);
        assert_eq!(palette.mode, PaletteMode::GotoLine);
    }

    #[test]
    fn parse_goto_line_only() {
        let result = Palette::parse_goto_line(":42");
        assert_eq!(result, Some((41, 0)));
    }

    #[test]
    fn parse_goto_line_and_char() {
        let result = Palette::parse_goto_line(":42:10");
        assert_eq!(result, Some((41, 9)));
    }

    #[test]
    fn parse_goto_line_empty() {
        assert_eq!(Palette::parse_goto_line(":"), None);
        assert_eq!(Palette::parse_goto_line(": "), None);
    }

    #[test]
    fn parse_goto_line_invalid() {
        assert_eq!(Palette::parse_goto_line(":abc"), None);
    }

    #[test]
    fn parse_goto_line_one_based_floor() {
        // Line 0 should not underflow
        let result = Palette::parse_goto_line(":0");
        assert_eq!(result, Some((0, 0)));
        // Line 1 is the first line (0-based: 0)
        let result = Palette::parse_goto_line(":1");
        assert_eq!(result, Some((0, 0)));
    }

    #[test]
    fn palette_symbol_filter_with_at_prefix() {
        let registry = CommandRegistry::new();
        let lang_registry = LanguageRegistry::new();
        let config = Config::default();
        let symbols = vec![
            ("main [function]  1:1".to_string(), 0, 0, vec![]),
            ("helper [function]  5:1".to_string(), 4, 0, vec![]),
        ];
        let mut palette = Palette::new(
            vec![],
            Path::new(""),
            &HashMap::new(),
            None,
            symbols,
            vec![],
        );
        palette.set_input("@hel".to_string());
        palette.update_candidates(&registry, &lang_registry, &config);
        assert_eq!(palette.mode, PaletteMode::SymbolPicker);
        assert_eq!(palette.candidates.len(), 1);
        assert!(palette.candidates[0].label.contains("helper"));
    }

    #[test]
    fn palette_unified_allows_prefix_deletion() {
        // Unified palettes allow cursor at position 0 so prefixes can be deleted
        let mut palette =
            Palette::new(vec![], Path::new(""), &HashMap::new(), None, vec![], vec![]);
        assert!(palette.is_unified);
        palette.set_input("@test".to_string());
        palette.input.cursor = 0;
        palette.clamp_input_cursor();
        assert_eq!(palette.input.cursor, 0);

        palette.set_input(":42".to_string());
        palette.input.cursor = 0;
        palette.clamp_input_cursor();
        assert_eq!(palette.input.cursor, 0);
    }

    #[test]
    fn palette_standalone_protects_prefix() {
        // Standalone symbol picker protects the prefix
        let palette =
            Palette::new_symbol_picker(vec![("main [function]  1:1".to_string(), 0, 0, vec![])]);
        assert!(!palette.is_unified);
        // Standalone symbol picker has no prefix in input, so min_input_cursor is 0
        assert_eq!(palette.min_input_cursor(), 0);
    }

    #[test]
    fn palette_goto_line_enter_dispatches_jump() {
        let registry = CommandRegistry::new();
        let lang_registry = LanguageRegistry::new();
        let config = Config::default();
        let doc_lines: Vec<String> = (0..50).map(|i| format!("line {}", i)).collect();
        let mut palette = Palette::new(
            vec![],
            Path::new(""),
            &HashMap::new(),
            None,
            vec![],
            doc_lines,
        );
        palette.set_input(":42".to_string());
        palette.update_candidates(&registry, &lang_registry, &config);

        let result = palette.handle_key_event(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &registry,
            &lang_registry,
            &config,
        );
        match result {
            EventResult::Action(Action::App(AppAction::Navigation(
                NavigationAction::JumpToLineChar { line, char_col },
            ))) => {
                assert_eq!(line, 41);
                assert_eq!(char_col, 0);
            }
            other => panic!("Expected JumpToLineChar, got {:?}", other),
        }
    }

    #[test]
    fn palette_goto_line_enter_invalid_closes() {
        let registry = CommandRegistry::new();
        let lang_registry = LanguageRegistry::new();
        let config = Config::default();
        let mut palette =
            Palette::new(vec![], Path::new(""), &HashMap::new(), None, vec![], vec![]);
        palette.set_input(":".to_string());
        palette.update_candidates(&registry, &lang_registry, &config);

        let result = palette.handle_key_event(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &registry,
            &lang_registry,
            &config,
        );
        match result {
            EventResult::Action(Action::Ui(UiAction::ClosePalette)) => {}
            other => panic!("Expected ClosePalette, got {:?}", other),
        }
    }

    #[test]
    fn palette_goto_line_preview_shows_context() {
        let registry = CommandRegistry::new();
        let lang_registry = LanguageRegistry::new();
        let config = Config::default();
        let doc_lines: Vec<String> = (0..20).map(|i| format!("content line {}", i + 1)).collect();
        let mut palette = Palette::new(
            vec![],
            Path::new(""),
            &HashMap::new(),
            None,
            vec![],
            doc_lines,
        );
        palette.set_input(":10".to_string());
        palette.update_candidates(&registry, &lang_registry, &config);

        assert!(!palette.preview_lines.is_empty());
        assert!(palette.preview_lines.iter().any(|l| l.contains("10 |")));
        assert!(palette.jump_target_preview_line.is_some());
    }

    #[test]
    fn palette_unified_mode_transition_via_typing() {
        let registry = CommandRegistry::new();
        let lang_registry = LanguageRegistry::new();
        let config = Config::default();
        let symbols = vec![("main [function]  1:1".to_string(), 0, 0, vec![])];
        let mut palette = Palette::new(
            vec!["test.rs".to_string()],
            Path::new(""),
            &HashMap::new(),
            None,
            symbols,
            vec!["hello".to_string()],
        );
        assert!(palette.is_unified);

        // Start as command mode (default input is ">")
        palette.update_candidates(&registry, &lang_registry, &config);
        assert_eq!(palette.mode, PaletteMode::Command);

        // Type @ via refresh_after_input_edit (simulating user input)
        palette.set_input("@".to_string());
        palette.refresh_after_input_edit(&registry, &lang_registry, &config);
        assert_eq!(palette.mode, PaletteMode::SymbolPicker);

        // Type : for goto line
        palette.set_input(":5".to_string());
        palette.refresh_after_input_edit(&registry, &lang_registry, &config);
        assert_eq!(palette.mode, PaletteMode::GotoLine);

        // Clear to file mode
        palette.set_input(String::new());
        palette.refresh_after_input_edit(&registry, &lang_registry, &config);
        assert_eq!(palette.mode, PaletteMode::FileFinder);
    }

    #[test]
    fn palette_backspace_deletes_prefix_and_switches_mode() {
        let registry = CommandRegistry::new();
        let lang_registry = LanguageRegistry::new();
        let config = Config::default();
        let symbols = vec![("main [function]  1:1".to_string(), 0, 0, vec![])];
        let mut palette = Palette::new(
            vec!["test.rs".to_string()],
            Path::new(""),
            &HashMap::new(),
            None,
            symbols,
            vec![],
        );

        // Start in symbol mode with "@"
        palette.set_input("@".to_string());
        palette.update_candidates(&registry, &lang_registry, &config);
        assert_eq!(palette.mode, PaletteMode::SymbolPicker);

        // Backspace removes "@", transitions to file picker
        palette.on_backspace(&registry, &lang_registry, &config);
        assert_eq!(palette.input.text, "");
        assert_eq!(palette.mode, PaletteMode::FileFinder);
    }
}
