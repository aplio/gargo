use std::io::{self, Write};

use crossterm::{
    cursor::{self, MoveTo, SetCursorStyle},
    event::{KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind},
    queue,
    terminal::{self, ClearType},
};

use crate::command::registry::CommandRegistry;
use crate::config::Config;
use crate::core::buffer::BufferId;
use crate::input::action::{
    Action, AppAction, CoreAction, IntegrationAction, UiAction, WindowDirection, WindowSplitAxis,
    WorkspaceAction,
};
use crate::input::chord::KeyState;
use crate::syntax::language::LanguageRegistry;
use crate::ui::framework::cell::CellStyle;
use crate::ui::framework::component::{Component, EventResult, RenderContext};
use crate::ui::framework::surface::Surface;
use crate::ui::framework::window_manager::{
    Direction, Divider, DividerOrientation, Layout, PaneRect, SplitAxis, WindowId, WindowManager,
};
use crate::ui::overlays::command_helper::CommandHelper;
use crate::ui::overlays::editor::find_replace::FindReplacePopup;
use crate::ui::overlays::editor::markdown_link_hover::{HoverKeyResult, MarkdownLinkHover};
use crate::ui::overlays::explorer::popup::ExplorerPopup;
use crate::ui::overlays::explorer::sidebar::Explorer;
use crate::ui::overlays::git::view::GitView;
use crate::ui::overlays::github::issue_picker::IssueListPicker;
use crate::ui::overlays::github::pr_picker::PrListPicker;
use crate::ui::overlays::palette::Palette;
use crate::ui::overlays::project::recent_picker::RecentProjectPopup;
use crate::ui::overlays::project::root_picker::ProjectRootPopup;
use crate::ui::overlays::project::save_as_popup::SaveAsPopup;
use crate::ui::views::notification_bar::NotificationBar;
use crate::ui::views::status_bar::StatusBar;
use crate::ui::views::text_view::TextView;
use crate::core_lib::text::input::TextInput;
use crate::core_lib::ui::text::display_width;

pub struct SearchBar {
    pub input: TextInput,
    pub saved_cursor: usize,
    pub saved_scroll: usize,
    pub saved_horizontal_scroll: usize,
}

impl SearchBar {
    /// Insert text at the current cursor position (for IME/paste support).
    pub fn insert_text(&mut self, text: &str) {
        self.input.insert_text(text);
    }
}

#[derive(Debug, Clone, Copy)]
struct MouseDividerDragState {
    primary_window_id: WindowId,
    secondary_window_id: WindowId,
    orientation: DividerOrientation,
    last_col: u16,
    last_row: u16,
}

pub struct Compositor {
    text_view: TextView,
    status_bar: StatusBar,
    notification_bar: NotificationBar,
    palette: Option<Palette>,
    git_view: Option<GitView>,
    pr_list_picker: Option<PrListPicker>,
    issue_list_picker: Option<IssueListPicker>,
    explorer_popup: Option<ExplorerPopup>,
    project_root_popup: Option<ProjectRootPopup>,
    recent_project_popup: Option<RecentProjectPopup>,
    save_as_popup: Option<SaveAsPopup>,
    find_replace_popup: Option<FindReplacePopup>,
    markdown_link_hover: Option<MarkdownLinkHover>,
    search_bar: Option<SearchBar>,
    explorer: Option<Explorer>,
    command_helper: Option<CommandHelper>,
    mouse_drag: Option<MouseDividerDragState>,
    window_manager: WindowManager,
    current: Surface,
    previous: Surface,
}

impl Default for Compositor {
    fn default() -> Self {
        Self::new()
    }
}

impl Compositor {
    pub fn new() -> Self {
        Self {
            text_view: TextView::new(),
            status_bar: StatusBar::new(),
            notification_bar: NotificationBar::new(),
            palette: None,
            git_view: None,
            pr_list_picker: None,
            issue_list_picker: None,
            explorer_popup: None,
            project_root_popup: None,
            recent_project_popup: None,
            save_as_popup: None,
            find_replace_popup: None,
            markdown_link_hover: None,
            search_bar: None,
            explorer: None,
            command_helper: None,
            mouse_drag: None,
            window_manager: WindowManager::new(1),
            current: Surface::new(0, 0),
            previous: Surface::new(0, 0),
        }
    }

    pub fn push_palette(&mut self, palette: Palette) {
        self.palette = Some(palette);
    }

    pub fn palette_mut(&mut self) -> Option<&mut Palette> {
        self.palette.as_mut()
    }

    pub fn search_bar_mut(&mut self) -> Option<&mut SearchBar> {
        self.search_bar.as_mut()
    }

    pub fn pop_palette(&mut self) -> Option<Palette> {
        self.palette.take()
    }

    pub fn set_markdown_link_hover_candidates(&mut self, candidates: Vec<String>) {
        if candidates.is_empty() {
            self.markdown_link_hover = None;
            return;
        }
        if let Some(hover) = self.markdown_link_hover.as_mut() {
            hover.set_candidates(candidates);
            if hover.is_empty() {
                self.markdown_link_hover = None;
            }
        } else {
            self.markdown_link_hover = Some(MarkdownLinkHover::new(candidates));
        }
    }

    pub fn close_markdown_link_hover(&mut self) {
        self.markdown_link_hover = None;
    }

    pub fn can_show_markdown_link_hover(&self) -> bool {
        self.palette.is_none()
            && self.git_view.is_none()
            && self.pr_list_picker.is_none()
            && self.issue_list_picker.is_none()
            && self.explorer_popup.is_none()
            && self.project_root_popup.is_none()
            && self.recent_project_popup.is_none()
            && self.save_as_popup.is_none()
            && self.find_replace_popup.is_none()
            && self.search_bar.is_none()
            && self.explorer.is_none()
    }

    pub fn open_git_view(&mut self, view: GitView) {
        self.git_view = Some(view);
    }

    pub fn git_view_mut(&mut self) -> Option<&mut GitView> {
        self.git_view.as_mut()
    }

    pub fn close_git_view(&mut self) {
        self.git_view = None;
    }

    pub fn open_pr_list_picker(&mut self, picker: PrListPicker) {
        self.pr_list_picker = Some(picker);
    }

    pub fn close_pr_list_picker(&mut self) {
        self.pr_list_picker = None;
    }

    pub fn open_issue_list_picker(&mut self, picker: IssueListPicker) {
        self.issue_list_picker = Some(picker);
    }

    pub fn close_issue_list_picker(&mut self) {
        self.issue_list_picker = None;
    }

    pub fn open_explorer_popup(&mut self, popup: ExplorerPopup) {
        self.explorer_popup = Some(popup);
    }

    pub fn explorer_popup_mut(&mut self) -> Option<&mut ExplorerPopup> {
        self.explorer_popup.as_mut()
    }

    pub fn close_explorer_popup(&mut self) {
        self.explorer_popup = None;
    }

    pub fn has_explorer_popup(&self) -> bool {
        self.explorer_popup.is_some()
    }

    pub fn open_project_root_popup(&mut self, popup: ProjectRootPopup) {
        self.project_root_popup = Some(popup);
    }

    pub fn close_project_root_popup(&mut self) {
        self.project_root_popup = None;
    }

    pub fn has_project_root_popup(&self) -> bool {
        self.project_root_popup.is_some()
    }

    pub fn open_recent_project_popup(&mut self, popup: RecentProjectPopup) {
        self.recent_project_popup = Some(popup);
    }

    pub fn close_recent_project_popup(&mut self) {
        self.recent_project_popup = None;
    }

    pub fn has_recent_project_popup(&self) -> bool {
        self.recent_project_popup.is_some()
    }

    pub fn open_save_as_popup(&mut self, popup: SaveAsPopup) {
        self.save_as_popup = Some(popup);
    }

    pub fn close_save_as_popup(&mut self) {
        self.save_as_popup = None;
    }

    pub fn has_save_as_popup(&self) -> bool {
        self.save_as_popup.is_some()
    }

    pub fn save_as_popup_input(&self) -> Option<&str> {
        self.save_as_popup.as_ref().map(|popup| popup.input())
    }

    pub fn open_find_replace_popup(&mut self, popup: FindReplacePopup) {
        self.find_replace_popup = Some(popup);
    }

    pub fn close_find_replace_popup(&mut self) {
        self.find_replace_popup = None;
    }

    pub fn find_replace_popup_mut(&mut self) -> Option<&mut FindReplacePopup> {
        self.find_replace_popup.as_mut()
    }

    pub fn update_command_helper(&mut self, key_state: &KeyState) {
        match key_state {
            KeyState::Normal => self.command_helper = None,
            _ => self.command_helper = Some(CommandHelper::new(key_state)),
        }
    }

    fn editor_rect_for_dims(&self, cols: usize, rows: usize) -> Option<PaneRect> {
        let height = rows.saturating_sub(2);
        if height == 0 {
            return None;
        }
        let (x, width) = self
            .explorer_layout(cols)
            .map(|(_, _, editor_x, editor_w)| (editor_x, editor_w))
            .unwrap_or((0, cols));
        if width == 0 {
            return None;
        }
        Some(PaneRect {
            x,
            y: 0,
            width,
            height,
        })
    }

    pub fn focused_pane_rect(&self, cols: usize, rows: usize) -> Option<PaneRect> {
        let area = self.editor_rect_for_dims(cols, rows)?;
        self.window_manager.focused_pane(area).map(|pane| pane.rect)
    }

    pub fn window_count(&self) -> usize {
        self.window_manager.window_count()
    }

    pub fn focused_buffer_id(&self) -> BufferId {
        self.window_manager.focused_buffer_id()
    }

    pub fn set_focused_buffer(&mut self, buffer_id: BufferId) {
        self.window_manager.set_focused_buffer(buffer_id);
    }

    pub fn replace_window_buffer_refs(&mut self, old_buffer_id: BufferId, new_buffer_id: BufferId) {
        self.window_manager
            .replace_buffer_refs(old_buffer_id, new_buffer_id);
    }

    pub fn split_focused_window(
        &mut self,
        axis: WindowSplitAxis,
        new_buffer_id: BufferId,
        cols: usize,
        rows: usize,
    ) -> Result<(), String> {
        let area = self
            .editor_rect_for_dims(cols, rows)
            .ok_or_else(|| "No editor area available".to_string())?;
        let focused = self
            .window_manager
            .focused_pane(area)
            .ok_or_else(|| "No focused window".to_string())?;
        match axis {
            WindowSplitAxis::Vertical => {
                if focused.rect.width < 3 {
                    return Err("Window too narrow to split".to_string());
                }
                self.window_manager
                    .split_focused(SplitAxis::Vertical, new_buffer_id);
            }
            WindowSplitAxis::Horizontal => {
                if focused.rect.height < 3 {
                    return Err("Window too short to split".to_string());
                }
                self.window_manager
                    .split_focused(SplitAxis::Horizontal, new_buffer_id);
            }
        }
        Ok(())
    }

    pub fn focus_window_direction(
        &mut self,
        direction: WindowDirection,
        cols: usize,
        rows: usize,
    ) -> Result<BufferId, String> {
        let area = self
            .editor_rect_for_dims(cols, rows)
            .ok_or_else(|| "No editor area available".to_string())?;
        self.window_manager
            .focus_direction(map_direction(direction), area)?;
        Ok(self.window_manager.focused_buffer_id())
    }

    pub fn focus_next_window(&mut self, cols: usize, rows: usize) -> Result<BufferId, String> {
        let area = self
            .editor_rect_for_dims(cols, rows)
            .ok_or_else(|| "No editor area available".to_string())?;
        self.window_manager.focus_next(area)?;
        Ok(self.window_manager.focused_buffer_id())
    }

    pub fn swap_window_direction(
        &mut self,
        direction: WindowDirection,
        cols: usize,
        rows: usize,
    ) -> Result<(), String> {
        let area = self
            .editor_rect_for_dims(cols, rows)
            .ok_or_else(|| "No editor area available".to_string())?;
        self.window_manager
            .swap_direction(map_direction(direction), area)
    }

    pub fn close_focused_window(&mut self) -> Result<BufferId, String> {
        self.window_manager.close_focused()?;
        Ok(self.window_manager.focused_buffer_id())
    }

    pub fn close_other_windows(&mut self) -> BufferId {
        self.window_manager.close_others();
        self.window_manager.focused_buffer_id()
    }

    pub fn render(&mut self, ctx: &RenderContext, stdout: &mut impl Write) -> io::Result<()> {
        if ctx
            .editor
            .buffer_by_id(self.window_manager.focused_buffer_id())
            .is_none()
        {
            self.window_manager
                .set_focused_buffer(ctx.editor.active_buffer().id);
        }

        let cols = ctx.cols;
        let rows = ctx.rows;

        // Resize buffers if terminal size changed
        if self.current.width != cols || self.current.height != rows {
            self.current.resize(cols, rows);
            self.previous.resize(cols, rows);
            // Clear terminal so stale content from the old layout doesn't persist.
            // After clear, the terminal screen is all blank, matching the all-default
            // `previous` buffer — so cells the diff skips are already blank on screen.
            queue!(stdout, terminal::Clear(ClearType::All))?;
        }

        // Clear current buffer
        self.current.reset();

        let layout = self.explorer_layout(cols);
        let is_fullscreen_explorer = layout.is_some() && cols < 80;

        // Render explorer if present
        if let (Some((ew, _border_col, _editor_x, _editor_w)), Some(explorer)) =
            (layout, &mut self.explorer)
        {
            let explorer_height = rows.saturating_sub(2); // stop before status bar
            explorer.render(&mut self.current, 0, ew, explorer_height);

            // Draw border column in split mode
            if cols >= 80 {
                let border_col = ew;
                let border_style = CellStyle {
                    dim: true,
                    ..CellStyle::default()
                };
                for r in 0..explorer_height {
                    self.current
                        .put_str(border_col, r, "\u{2502}", &border_style);
                }
            }
        }

        // Render editor panes unless explorer is fullscreen
        if !is_fullscreen_explorer {
            self.render_windows(ctx);
        }

        // Status bar always renders full width
        self.status_bar.render(ctx, &mut self.current);

        // Add notification bar below status bar
        self.notification_bar.render(ctx, &mut self.current);

        // Render command helper if active (after notification_bar, before overlays)
        if let Some(ref helper) = self.command_helper {
            helper.render_overlay(&mut self.current, cols, rows, ctx.theme);
        }

        // Search bar overlay on status row
        if let Some(ref bar) = self.search_bar {
            let status_row = rows.saturating_sub(1);
            let prompt = format!("/{}", bar.input.text);
            let reverse_style = CellStyle {
                reverse: true,
                ..CellStyle::default()
            };
            // Clear the status row and draw search prompt
            self.current
                .fill_region(0, status_row, cols, ' ', &reverse_style);
            self.current.put_str(0, status_row, &prompt, &reverse_style);

            // Draw diff between previous and current
            draw_diff(&self.previous, &self.current, stdout)?;

            // Position cursor at bar.input.cursor within the input
            let before_cursor = &bar.input.text[..bar.input.byte_index_at_cursor()];
            let cursor_x = (1 + display_width(before_cursor)) as u16;
            let cursor_y = status_row as u16;
            queue!(stdout, MoveTo(cursor_x, cursor_y))?;
            queue!(stdout, SetCursorStyle::BlinkingBar)?;
            queue!(stdout, cursor::Show)?;
            stdout.flush()?;

            // Swap buffers
            std::mem::swap(&mut self.current, &mut self.previous);
            return Ok(());
        }

        if let Some(ref mut git_view) = self.git_view {
            let cursor = git_view.render_overlay(&mut self.current);

            draw_diff(&self.previous, &self.current, stdout)?;

            if let Some((cx, cy)) = cursor {
                queue!(stdout, MoveTo(cx, cy))?;
                queue!(stdout, SetCursorStyle::BlinkingBar)?;
                queue!(stdout, cursor::Show)?;
            } else {
                queue!(stdout, cursor::Hide)?;
            }
            stdout.flush()?;

            std::mem::swap(&mut self.current, &mut self.previous);
            return Ok(());
        }

        if let Some(ref mut pr_list_picker) = self.pr_list_picker {
            let cursor = pr_list_picker.render_overlay(&mut self.current);

            draw_diff(&self.previous, &self.current, stdout)?;

            if let Some((cx, cy)) = cursor {
                queue!(stdout, MoveTo(cx, cy))?;
                queue!(stdout, SetCursorStyle::BlinkingBar)?;
                queue!(stdout, cursor::Show)?;
            } else {
                queue!(stdout, cursor::Hide)?;
            }
            stdout.flush()?;

            std::mem::swap(&mut self.current, &mut self.previous);
            return Ok(());
        }

        if let Some(ref mut issue_list_picker) = self.issue_list_picker {
            let cursor = issue_list_picker.render_overlay(&mut self.current);

            draw_diff(&self.previous, &self.current, stdout)?;

            if let Some((cx, cy)) = cursor {
                queue!(stdout, MoveTo(cx, cy))?;
                queue!(stdout, SetCursorStyle::BlinkingBar)?;
                queue!(stdout, cursor::Show)?;
            } else {
                queue!(stdout, cursor::Hide)?;
            }
            stdout.flush()?;

            std::mem::swap(&mut self.current, &mut self.previous);
            return Ok(());
        }

        if let Some(ref mut find_replace_popup) = self.find_replace_popup {
            let cursor = find_replace_popup.render_overlay(&mut self.current);

            draw_diff(&self.previous, &self.current, stdout)?;

            if let Some((cx, cy)) = cursor {
                queue!(stdout, MoveTo(cx, cy))?;
                queue!(stdout, SetCursorStyle::BlinkingBar)?;
                queue!(stdout, cursor::Show)?;
            } else {
                queue!(stdout, cursor::Hide)?;
            }
            stdout.flush()?;

            std::mem::swap(&mut self.current, &mut self.previous);
            return Ok(());
        }

        if let Some(ref project_root_popup) = self.project_root_popup {
            let cursor = project_root_popup.render_overlay(&mut self.current);

            draw_diff(&self.previous, &self.current, stdout)?;

            if let Some((cx, cy)) = cursor {
                queue!(stdout, MoveTo(cx, cy))?;
                queue!(stdout, SetCursorStyle::BlinkingBar)?;
                queue!(stdout, cursor::Show)?;
            } else {
                queue!(stdout, cursor::Hide)?;
            }
            stdout.flush()?;

            std::mem::swap(&mut self.current, &mut self.previous);
            return Ok(());
        }

        if let Some(ref recent_project_popup) = self.recent_project_popup {
            let cursor = recent_project_popup.render_overlay(&mut self.current);
            draw_diff(&self.previous, &self.current, stdout)?;

            if let Some((cx, cy)) = cursor {
                queue!(stdout, MoveTo(cx, cy))?;
                queue!(stdout, SetCursorStyle::BlinkingBar)?;
                queue!(stdout, cursor::Show)?;
            } else {
                queue!(stdout, cursor::Hide)?;
            }
            stdout.flush()?;

            std::mem::swap(&mut self.current, &mut self.previous);
            return Ok(());
        }

        if let Some(ref save_as_popup) = self.save_as_popup {
            let cursor = save_as_popup.render_overlay(&mut self.current);

            draw_diff(&self.previous, &self.current, stdout)?;

            if let Some((cx, cy)) = cursor {
                queue!(stdout, MoveTo(cx, cy))?;
                queue!(stdout, SetCursorStyle::BlinkingBar)?;
                queue!(stdout, cursor::Show)?;
            } else {
                queue!(stdout, cursor::Hide)?;
            }
            stdout.flush()?;

            std::mem::swap(&mut self.current, &mut self.previous);
            return Ok(());
        }

        if let Some(ref mut explorer_popup) = self.explorer_popup {
            let cursor = explorer_popup.render_overlay(&mut self.current, ctx.theme);

            draw_diff(&self.previous, &self.current, stdout)?;

            if let Some((cx, cy)) = cursor {
                queue!(stdout, MoveTo(cx, cy))?;
                queue!(stdout, SetCursorStyle::BlinkingBar)?;
                queue!(stdout, cursor::Show)?;
            } else {
                queue!(stdout, cursor::Hide)?;
            }
            stdout.flush()?;

            std::mem::swap(&mut self.current, &mut self.previous);
            return Ok(());
        }

        if let Some(ref mut palette) = self.palette {
            let (cx, cy) = palette.render_overlay(&mut self.current, ctx.theme);

            // Draw diff between previous and current
            draw_diff(&self.previous, &self.current, stdout)?;

            queue!(stdout, MoveTo(cx, cy))?;
            queue!(stdout, SetCursorStyle::BlinkingBar)?;
            queue!(stdout, cursor::Show)?;
            stdout.flush()?;

            // Swap buffers
            std::mem::swap(&mut self.current, &mut self.previous);
            return Ok(());
        }

        if let Some(ref hover) = self.markdown_link_hover
            && let Some((cursor_x, cursor_y, _)) = self.focused_window_cursor(ctx)
        {
            hover.render_overlay(
                &mut self.current,
                cursor_x as usize,
                cursor_y as usize,
                ctx.theme,
            );
        }

        // Draw diff between previous and current
        draw_diff(&self.previous, &self.current, stdout)?;

        // Handle cursor: explorer find mode cursor takes priority when explorer is present
        if let Some(ref explorer) = self.explorer {
            let explorer_height = rows.saturating_sub(2);
            if let Some((cx, cy)) = explorer.find_cursor(0, explorer_height) {
                queue!(stdout, MoveTo(cx, cy))?;
                queue!(stdout, SetCursorStyle::BlinkingBar)?;
                queue!(stdout, cursor::Show)?;
            } else if !is_fullscreen_explorer {
                // Explorer is open but not in find mode: show focused pane cursor
                if let Some((col, row, style)) = self.focused_window_cursor(ctx) {
                    queue!(stdout, MoveTo(col, row))?;
                    queue!(stdout, style)?;
                    queue!(stdout, cursor::Show)?;
                } else {
                    queue!(stdout, cursor::Hide)?;
                }
            } else {
                queue!(stdout, cursor::Hide)?;
            }
        } else {
            // No explorer: focused pane cursor
            if let Some((col, row, style)) = self.focused_window_cursor(ctx) {
                queue!(stdout, MoveTo(col, row))?;
                queue!(stdout, style)?;
                queue!(stdout, cursor::Show)?;
            } else {
                queue!(stdout, cursor::Hide)?;
            }
        }

        stdout.flush()?;

        // Swap buffers
        std::mem::swap(&mut self.current, &mut self.previous);
        Ok(())
    }

    /// Handle key event. If palette or search bar is active, it gets priority.
    pub fn handle_key(
        &mut self,
        key: KeyEvent,
        registry: &CommandRegistry,
        lang_registry: &LanguageRegistry,
        config: &Config,
        key_state: &KeyState,
    ) -> EventResult {
        if let Some(ref mut palette) = self.palette {
            return palette.handle_key_event(key, registry, lang_registry, config);
        }
        if let Some(ref mut find_replace_popup) = self.find_replace_popup {
            return find_replace_popup.handle_key(key);
        }
        if let Some(ref mut git_view) = self.git_view {
            return git_view.handle_key(key);
        }
        if let Some(ref mut pr_list_picker) = self.pr_list_picker {
            return pr_list_picker.handle_key(key);
        }
        if let Some(ref mut issue_list_picker) = self.issue_list_picker {
            return issue_list_picker.handle_key(key);
        }
        if let Some(ref mut project_root_popup) = self.project_root_popup {
            return project_root_popup.handle_key(key);
        }
        if let Some(ref mut recent_project_popup) = self.recent_project_popup {
            return recent_project_popup.handle_key(key);
        }
        if let Some(ref mut save_as_popup) = self.save_as_popup {
            return save_as_popup.handle_key(key);
        }
        if let Some(ref mut explorer_popup) = self.explorer_popup {
            return explorer_popup.handle_key(key);
        }
        if let Some(ref mut bar) = self.search_bar {
            if key
                .modifiers
                .contains(crossterm::event::KeyModifiers::CONTROL)
            {
                return match key.code {
                    KeyCode::Char('q') => {
                        let saved_cursor = bar.saved_cursor;
                        let saved_scroll = bar.saved_scroll;
                        let saved_horizontal_scroll = bar.saved_horizontal_scroll;
                        EventResult::Action(Action::App(AppAction::Workspace(
                            WorkspaceAction::SearchCancel {
                                saved_cursor,
                                saved_scroll,
                                saved_horizontal_scroll,
                            },
                        )))
                    }
                    KeyCode::Char('p') => EventResult::Action(Action::App(AppAction::Workspace(
                        WorkspaceAction::SearchHistoryPrev,
                    ))),
                    KeyCode::Char('n') => EventResult::Action(Action::App(AppAction::Workspace(
                        WorkspaceAction::SearchHistoryNext,
                    ))),
                    KeyCode::Char('a') => {
                        bar.input.move_start();
                        EventResult::Consumed
                    }
                    KeyCode::Char('e') => {
                        bar.input.move_end();
                        EventResult::Consumed
                    }
                    KeyCode::Char('f') => {
                        bar.input.move_right();
                        EventResult::Consumed
                    }
                    KeyCode::Char('b') => {
                        bar.input.move_left();
                        EventResult::Consumed
                    }
                    KeyCode::Char('k') => {
                        let _ = bar.input.delete_to_end();
                        EventResult::Action(Action::Core(CoreAction::SearchUpdate(
                            bar.input.text.clone(),
                        )))
                    }
                    KeyCode::Char('w') => {
                        let _ = bar.input.delete_prev_word();
                        EventResult::Action(Action::Core(CoreAction::SearchUpdate(
                            bar.input.text.clone(),
                        )))
                    }
                    _ => EventResult::Consumed,
                };
            }
            return match key.code {
                KeyCode::Up => EventResult::Action(Action::App(AppAction::Workspace(
                    WorkspaceAction::SearchHistoryPrev,
                ))),
                KeyCode::Down => EventResult::Action(Action::App(AppAction::Workspace(
                    WorkspaceAction::SearchHistoryNext,
                ))),
                KeyCode::Left => {
                    bar.input.move_left();
                    EventResult::Consumed
                }
                KeyCode::Right => {
                    bar.input.move_right();
                    EventResult::Consumed
                }
                KeyCode::Esc => {
                    let saved_cursor = bar.saved_cursor;
                    let saved_scroll = bar.saved_scroll;
                    let saved_horizontal_scroll = bar.saved_horizontal_scroll;
                    EventResult::Action(Action::App(AppAction::Workspace(
                        WorkspaceAction::SearchCancel {
                            saved_cursor,
                            saved_scroll,
                            saved_horizontal_scroll,
                        },
                    )))
                }
                KeyCode::Enter => EventResult::Action(Action::App(AppAction::Workspace(
                    WorkspaceAction::SearchConfirm,
                ))),
                KeyCode::Backspace => {
                    let _ = bar.input.backspace();
                    EventResult::Action(Action::Core(CoreAction::SearchUpdate(
                        bar.input.text.clone(),
                    )))
                }
                KeyCode::Char(c) => {
                    bar.input.insert_char(c);
                    EventResult::Action(Action::Core(CoreAction::SearchUpdate(
                        bar.input.text.clone(),
                    )))
                }
                _ => EventResult::Consumed,
            };
        }
        if let Some(ref mut hover) = self.markdown_link_hover {
            match hover.handle_key(key) {
                HoverKeyResult::Ignored => {}
                HoverKeyResult::Consumed => return EventResult::Consumed,
                HoverKeyResult::Close => {
                    self.markdown_link_hover = None;
                    return EventResult::Consumed;
                }
                HoverKeyResult::Apply(candidate) => {
                    return EventResult::Action(Action::App(AppAction::Integration(
                        IntegrationAction::ApplyMarkdownLinkCompletion { candidate },
                    )));
                }
            }
        }
        if let Some(ref mut explorer) = self.explorer {
            let result = explorer.handle_key(key, key_state);
            if !matches!(result, EventResult::Ignored) {
                return result;
            }
        }
        EventResult::Ignored
    }

    fn event_surface_size(&self) -> (usize, usize) {
        let cols = self.current.width.max(self.previous.width);
        let rows = self.current.height.max(self.previous.height);
        if cols == 0 || rows == 0 {
            (80, 24)
        } else {
            (cols, rows)
        }
    }

    fn window_layout_for_event_dims(&self, cols: usize, rows: usize) -> Option<Layout> {
        let area = self.editor_rect_for_dims(cols, rows)?;
        Some(self.window_manager.layout(area))
    }

    fn has_modal_mouse_overlay(&self) -> bool {
        self.palette.is_some()
            || self.git_view.is_some()
            || self.pr_list_picker.is_some()
            || self.issue_list_picker.is_some()
            || self.explorer_popup.is_some()
            || self.project_root_popup.is_some()
            || self.recent_project_popup.is_some()
            || self.save_as_popup.is_some()
            || self.find_replace_popup.is_some()
            || self.search_bar.is_some()
    }

    fn mouse_divider_at(layout: &Layout, col: u16, row: u16) -> Option<Divider> {
        let col = usize::from(col);
        let row = usize::from(row);
        layout
            .dividers
            .iter()
            .copied()
            .find(|divider| match divider.orientation {
                DividerOrientation::Vertical => {
                    col == divider.x && row >= divider.y && row < divider.y + divider.len
                }
                DividerOrientation::Horizontal => {
                    row == divider.y && col >= divider.x && col < divider.x + divider.len
                }
            })
    }

    fn mouse_windows_for_divider(
        layout: &Layout,
        divider: Divider,
        mouse_col: u16,
        mouse_row: u16,
    ) -> Option<(WindowId, WindowId)> {
        let mouse_col = usize::from(mouse_col);
        let mouse_row = usize::from(mouse_row);
        match divider.orientation {
            DividerOrientation::Vertical => {
                let primary = layout.panes.iter().find(|pane| {
                    pane.rect.x + pane.rect.width == divider.x
                        && mouse_row >= pane.rect.y
                        && mouse_row < pane.rect.y + pane.rect.height
                })?;
                let secondary = layout.panes.iter().find(|pane| {
                    pane.rect.x == divider.x + 1
                        && mouse_row >= pane.rect.y
                        && mouse_row < pane.rect.y + pane.rect.height
                })?;
                Some((primary.window_id, secondary.window_id))
            }
            DividerOrientation::Horizontal => {
                let primary = layout.panes.iter().find(|pane| {
                    pane.rect.y + pane.rect.height == divider.y
                        && mouse_col >= pane.rect.x
                        && mouse_col < pane.rect.x + pane.rect.width
                })?;
                let secondary = layout.panes.iter().find(|pane| {
                    pane.rect.y == divider.y + 1
                        && mouse_col >= pane.rect.x
                        && mouse_col < pane.rect.x + pane.rect.width
                })?;
                Some((primary.window_id, secondary.window_id))
            }
        }
    }

    pub fn handle_mouse(&mut self, mouse: &MouseEvent) -> EventResult {
        let (cols, rows) = self.event_surface_size();
        match mouse.kind {
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                self.mouse_drag = None;
                if let Some(ref mut git_view) = self.git_view {
                    let result = git_view.handle_mouse_scroll(mouse.kind, cols, rows);
                    if !matches!(result, EventResult::Ignored) {
                        return result;
                    }
                }

                if let Some(ref mut pr_list_picker) = self.pr_list_picker {
                    let result = pr_list_picker.handle_mouse_scroll(mouse.kind, cols, rows);
                    if !matches!(result, EventResult::Ignored) {
                        return result;
                    }
                }

                if let Some(ref mut issue_list_picker) = self.issue_list_picker {
                    let result = issue_list_picker.handle_mouse_scroll(mouse.kind, cols, rows);
                    if !matches!(result, EventResult::Ignored) {
                        return result;
                    }
                }

                if let Some(ref mut explorer_popup) = self.explorer_popup {
                    let result = explorer_popup.handle_mouse_scroll(mouse.kind, cols, rows);
                    if !matches!(result, EventResult::Ignored) {
                        return result;
                    }
                }

                EventResult::Ignored
            }
            MouseEventKind::Down(MouseButton::Left) => {
                self.mouse_drag = None;
                if self.has_modal_mouse_overlay() {
                    return EventResult::Ignored;
                }
                let Some(layout) = self.window_layout_for_event_dims(cols, rows) else {
                    return EventResult::Ignored;
                };
                let Some(divider) = Self::mouse_divider_at(&layout, mouse.column, mouse.row) else {
                    return EventResult::Ignored;
                };
                let Some((primary_window_id, secondary_window_id)) =
                    Self::mouse_windows_for_divider(&layout, divider, mouse.column, mouse.row)
                else {
                    return EventResult::Ignored;
                };

                self.mouse_drag = Some(MouseDividerDragState {
                    primary_window_id,
                    secondary_window_id,
                    orientation: divider.orientation,
                    last_col: mouse.column,
                    last_row: mouse.row,
                });
                EventResult::Consumed
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                if self.has_modal_mouse_overlay() {
                    self.mouse_drag = None;
                    return EventResult::Ignored;
                }

                let Some(mut drag) = self.mouse_drag.take() else {
                    return EventResult::Ignored;
                };

                let delta_col = i32::from(mouse.column) - i32::from(drag.last_col);
                let delta_row = i32::from(mouse.row) - i32::from(drag.last_row);
                let delta = match drag.orientation {
                    DividerOrientation::Vertical => {
                        if delta_col == 0 {
                            self.mouse_drag = Some(drag);
                            return EventResult::Consumed;
                        }
                        delta_col as i16
                    }
                    DividerOrientation::Horizontal => {
                        if delta_row == 0 {
                            self.mouse_drag = Some(drag);
                            return EventResult::Consumed;
                        }
                        delta_row as i16
                    }
                };

                // Keep drag state even when resize is clamped/no-op so the same
                // gesture can recover as soon as pointer motion reverses.
                let _ = self.window_manager.resize_between_windows(
                    drag.primary_window_id,
                    drag.secondary_window_id,
                    drag.orientation,
                    delta,
                );
                drag.last_col = mouse.column;
                drag.last_row = mouse.row;
                self.mouse_drag = Some(drag);
                EventResult::Consumed
            }
            MouseEventKind::Up(MouseButton::Left) => {
                if self.mouse_drag.take().is_some() {
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            _ => EventResult::Ignored,
        }
    }

    pub fn apply(&mut self, action: UiAction) {
        match action {
            UiAction::ClosePalette => {
                self.pop_palette();
            }
            UiAction::CloseExplorerPopup => {
                self.close_explorer_popup();
            }
            UiAction::CloseProjectRootPopup => {
                self.close_project_root_popup();
            }
            UiAction::CloseRecentProjectPopup => {
                self.close_recent_project_popup();
            }
            UiAction::CloseSaveAsPopup => {
                self.close_save_as_popup();
            }
            UiAction::CloseGitView => {
                self.close_git_view();
            }
            UiAction::ClosePrListPicker => {
                self.close_pr_list_picker();
            }
            UiAction::CloseIssueListPicker => {
                self.close_issue_list_picker();
            }
            UiAction::CloseFindReplacePopup => {
                self.close_find_replace_popup();
            }
            UiAction::OpenSearchBar {
                saved_cursor,
                saved_scroll,
                saved_horizontal_scroll,
            } => {
                self.open_search_bar(saved_cursor, saved_scroll, saved_horizontal_scroll);
            }
            UiAction::CloseSearchBar => {
                self.close_search_bar();
            }
            UiAction::SetSearchBarInput(input) => {
                self.set_search_bar_input(input);
            }
        }
    }

    pub fn open_search_bar(
        &mut self,
        saved_cursor: usize,
        saved_scroll: usize,
        saved_horizontal_scroll: usize,
    ) {
        self.search_bar = Some(SearchBar {
            input: TextInput::default(),
            saved_cursor,
            saved_scroll,
            saved_horizontal_scroll,
        });
    }

    pub fn close_search_bar(&mut self) {
        self.search_bar = None;
    }

    /// Update the search bar's input text (used when recalling history).
    pub fn set_search_bar_input(&mut self, input: String) {
        if let Some(ref mut bar) = self.search_bar {
            bar.input.set_text(input);
        }
    }

    /// Get the current search bar input, if the search bar is open.
    pub fn search_bar_input(&self) -> Option<&str> {
        self.search_bar.as_ref().map(|bar| bar.input.text.as_str())
    }

    pub fn open_explorer(&mut self, explorer: Explorer) {
        self.explorer = Some(explorer);
    }

    pub fn explorer_mut(&mut self) -> Option<&mut Explorer> {
        self.explorer.as_mut()
    }

    pub fn close_explorer(&mut self) -> Option<Explorer> {
        self.explorer.take()
    }

    pub fn has_explorer(&self) -> bool {
        self.explorer.is_some()
    }

    /// Returns (explorer_width, border_col, editor_x, editor_width) if explorer is open.
    /// In split mode (cols >= 80): explorer gets 30 cols, border at col 30, editor starts at 31.
    /// In fullscreen mode (cols < 80): explorer takes full width, no editor.
    pub fn explorer_layout(&self, cols: usize) -> Option<(usize, usize, usize, usize)> {
        self.explorer.as_ref()?;
        if cols >= 80 {
            let ew = 30;
            let border_col = ew;
            let editor_x = ew + 1;
            let editor_w = cols.saturating_sub(editor_x);
            Some((ew, border_col, editor_x, editor_w))
        } else {
            // Fullscreen: explorer takes all cols, no editor visible
            Some((cols, 0, 0, 0))
        }
    }

    fn render_windows(&mut self, ctx: &RenderContext) {
        let Some(area) = self.editor_rect_for_dims(ctx.cols, ctx.rows) else {
            return;
        };
        let layout = self.window_manager.layout(area);
        let focused_window = self.window_manager.focused_window_id();
        let active_buffer_id = ctx.editor.active_buffer().id;

        for pane in layout.panes {
            let Some(buffer) = ctx.editor.buffer_by_id(pane.buffer_id) else {
                continue;
            };
            let is_focused = pane.window_id == focused_window;
            let show_home = ctx.home_screen_active && is_focused && buffer.id == active_buffer_id;
            let show_search = is_focused && buffer.id == active_buffer_id;
            self.text_view.render_buffer(
                ctx,
                &mut self.current,
                buffer,
                pane.rect,
                show_search,
                show_home,
            );
        }

        let divider_style = CellStyle {
            dim: true,
            ..CellStyle::default()
        };
        for divider in layout.dividers {
            match divider.orientation {
                DividerOrientation::Vertical => {
                    for row in 0..divider.len {
                        self.current.put_str(
                            divider.x,
                            divider.y + row,
                            "\u{2502}",
                            &divider_style,
                        );
                    }
                }
                DividerOrientation::Horizontal => {
                    for col in 0..divider.len {
                        self.current.put_str(
                            divider.x + col,
                            divider.y,
                            "\u{2500}",
                            &divider_style,
                        );
                    }
                }
            }
        }
    }

    fn focused_window_cursor(&self, ctx: &RenderContext) -> Option<(u16, u16, SetCursorStyle)> {
        let area = self.editor_rect_for_dims(ctx.cols, ctx.rows)?;
        let focused = self.window_manager.focused_pane(area)?;
        let buffer = ctx.editor.buffer_by_id(focused.buffer_id)?;
        let show_home = ctx.home_screen_active && buffer.id == ctx.editor.active_buffer().id;
        self.text_view
            .cursor_for_buffer(ctx, buffer, focused.rect, show_home)
    }
}

fn map_direction(direction: WindowDirection) -> Direction {
    match direction {
        WindowDirection::Left => Direction::Left,
        WindowDirection::Down => Direction::Down,
        WindowDirection::Up => Direction::Up,
        WindowDirection::Right => Direction::Right,
    }
}

fn draw_diff(prev: &Surface, curr: &Surface, stdout: &mut impl Write) -> io::Result<()> {
    crate::core_lib::ui::diff::draw_diff(prev, curr, stdout)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::registry::CommandRegistry;
    use crate::config::Config;
    use crate::input::action::{Action, AppAction, BufferAction};
    use crate::input::chord::KeyState;
    use crate::syntax::language::LanguageRegistry;
    use crate::ui::framework::component::EventResult;
    use crate::ui::overlays::explorer::popup::ExplorerPopup;
    use crossterm::event::{
        KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    };
    use std::collections::HashMap;
    use std::fs;
    use std::path::PathBuf;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn mouse(kind: MouseEventKind) -> MouseEvent {
        mouse_at(kind, 0, 0)
    }

    fn mouse_at(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
        MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        }
    }

    fn setup_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("kaguya_test_comp_{}", name));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::create_dir_all(dir.join("aaa_dir")).unwrap();
        fs::write(dir.join("bbb.txt"), "bbb").unwrap();
        dir
    }

    fn cleanup(dir: &PathBuf) {
        let _ = fs::remove_dir_all(dir);
    }

    /// After popup opens a file and is closed, keys must fall through
    /// to EventResult::Ignored so the keymap can process them.
    #[test]
    fn keys_fall_through_after_popup_closed() {
        let dir = setup_dir("close");
        let registry = CommandRegistry::new();
        let lang_registry = LanguageRegistry::new();
        let config = Config::default();
        let key_state = KeyState::Normal;

        let mut comp = Compositor::new();
        comp.open_explorer_popup(ExplorerPopup::new(dir.clone(), &HashMap::new()));

        // Navigate to bbb.txt (index 1 — past the aaa_dir)
        let r = comp.handle_key(
            key(KeyCode::Char('j')),
            &registry,
            &lang_registry,
            &config,
            &key_state,
        );
        assert!(matches!(r, EventResult::Consumed));

        // Press Enter → file should produce an OpenFileFromExplorerPopup
        let r = comp.handle_key(
            key(KeyCode::Enter),
            &registry,
            &lang_registry,
            &config,
            &key_state,
        );
        match r {
            EventResult::Action(Action::App(AppAction::Buffer(
                BufferAction::OpenFileFromExplorerPopup(ref path),
            ))) => {
                assert!(path.ends_with("bbb.txt"));
            }
            _ => panic!("Expected OpenFileFromExplorerPopup from popup, got something else"),
        }

        // Simulate what app.rs dispatch does
        comp.close_explorer_popup();
        assert!(!comp.has_explorer_popup());

        // Subsequent key must NOT be consumed — it should reach the keymap
        let r = comp.handle_key(
            key(KeyCode::Char('j')),
            &registry,
            &lang_registry,
            &config,
            &key_state,
        );
        assert!(
            matches!(r, EventResult::Ignored),
            "After popup closed, key should be Ignored (pass to keymap)"
        );

        cleanup(&dir);
    }

    /// While popup is active, ALL keys should be intercepted (Consumed or Action).
    #[test]
    fn popup_intercepts_all_keys() {
        let dir = setup_dir("intercept");
        let registry = CommandRegistry::new();
        let lang_registry = LanguageRegistry::new();
        let config = Config::default();
        let key_state = KeyState::Normal;

        let mut comp = Compositor::new();
        comp.open_explorer_popup(ExplorerPopup::new(dir.clone(), &HashMap::new()));

        // Random keys should all be consumed or produce actions, never Ignored
        for code in [
            KeyCode::Char('x'),
            KeyCode::Char('q'),
            KeyCode::Tab,
            KeyCode::F(5),
        ] {
            let r = comp.handle_key(key(code), &registry, &lang_registry, &config, &key_state);
            assert!(
                !matches!(r, EventResult::Ignored),
                "Popup should intercept {:?}",
                code,
            );
        }

        cleanup(&dir);
    }

    #[test]
    fn mouse_scroll_ignored_without_overlay() {
        let mut comp = Compositor::new();
        let result = comp.handle_mouse(&mouse(MouseEventKind::ScrollDown));
        assert!(matches!(result, EventResult::Ignored));
    }

    #[test]
    fn mouse_scroll_consumed_by_issue_overlay() {
        let mut comp = Compositor::new();
        comp.open_issue_list_picker(
            crate::ui::overlays::github::issue_picker::IssueListPicker::new(vec![]),
        );
        let result = comp.handle_mouse(&mouse(MouseEventKind::ScrollDown));
        assert!(matches!(result, EventResult::Consumed));
    }

    #[test]
    fn mouse_drag_vertical_divider_resizes_windows() {
        let mut comp = Compositor::new();
        comp.window_manager.split_focused(SplitAxis::Vertical, 2);

        let before = comp
            .window_layout_for_event_dims(80, 24)
            .expect("layout before");
        let divider = before
            .dividers
            .iter()
            .find(|divider| divider.orientation == DividerOrientation::Vertical)
            .copied()
            .expect("vertical divider");
        let (primary_window, _) = Compositor::mouse_windows_for_divider(
            &before,
            divider,
            divider.x as u16,
            divider.y as u16,
        )
        .expect("divider windows");
        let anchor_width_before = before
            .panes
            .iter()
            .find(|pane| pane.window_id == primary_window)
            .expect("anchor pane before")
            .rect
            .width;

        let down = comp.handle_mouse(&mouse_at(
            MouseEventKind::Down(MouseButton::Left),
            divider.x as u16,
            divider.y as u16,
        ));
        assert!(matches!(down, EventResult::Consumed));
        let drag = comp.handle_mouse(&mouse_at(
            MouseEventKind::Drag(MouseButton::Left),
            divider.x.saturating_add(3) as u16,
            divider.y as u16,
        ));
        assert!(matches!(drag, EventResult::Consumed));
        let up = comp.handle_mouse(&mouse_at(
            MouseEventKind::Up(MouseButton::Left),
            divider.x.saturating_add(3) as u16,
            divider.y as u16,
        ));
        assert!(matches!(up, EventResult::Consumed));

        let after = comp
            .window_layout_for_event_dims(80, 24)
            .expect("layout after");
        let anchor_width_after = after
            .panes
            .iter()
            .find(|pane| pane.window_id == primary_window)
            .expect("anchor pane after")
            .rect
            .width;
        assert!(anchor_width_after > anchor_width_before);
    }

    #[test]
    fn mouse_drag_horizontal_divider_resizes_windows() {
        let mut comp = Compositor::new();
        comp.window_manager.split_focused(SplitAxis::Horizontal, 2);

        let before = comp
            .window_layout_for_event_dims(80, 24)
            .expect("layout before");
        let divider = before
            .dividers
            .iter()
            .find(|divider| divider.orientation == DividerOrientation::Horizontal)
            .copied()
            .expect("horizontal divider");
        let (primary_window, _) = Compositor::mouse_windows_for_divider(
            &before,
            divider,
            divider.x as u16,
            divider.y as u16,
        )
        .expect("divider windows");
        let anchor_height_before = before
            .panes
            .iter()
            .find(|pane| pane.window_id == primary_window)
            .expect("anchor pane before")
            .rect
            .height;

        let down = comp.handle_mouse(&mouse_at(
            MouseEventKind::Down(MouseButton::Left),
            divider.x as u16,
            divider.y as u16,
        ));
        assert!(matches!(down, EventResult::Consumed));
        let drag = comp.handle_mouse(&mouse_at(
            MouseEventKind::Drag(MouseButton::Left),
            divider.x as u16,
            divider.y.saturating_add(3) as u16,
        ));
        assert!(matches!(drag, EventResult::Consumed));
        let up = comp.handle_mouse(&mouse_at(
            MouseEventKind::Up(MouseButton::Left),
            divider.x as u16,
            divider.y.saturating_add(3) as u16,
        ));
        assert!(matches!(up, EventResult::Consumed));

        let after = comp
            .window_layout_for_event_dims(80, 24)
            .expect("layout after");
        let anchor_height_after = after
            .panes
            .iter()
            .find(|pane| pane.window_id == primary_window)
            .expect("anchor pane after")
            .rect
            .height;
        assert!(anchor_height_after > anchor_height_before);
    }

    #[test]
    fn mouse_drag_outer_divider_resizes_outer_split_with_nested_vertical_tree() {
        let mut comp = Compositor::new();
        comp.window_manager.split_focused(SplitAxis::Vertical, 2);
        comp.window_manager
            .focus_direction(
                Direction::Left,
                PaneRect {
                    x: 0,
                    y: 0,
                    width: 80,
                    height: 24,
                },
            )
            .expect("focus left window");
        comp.window_manager.split_focused(SplitAxis::Vertical, 3);

        let before = comp
            .window_layout_for_event_dims(80, 24)
            .expect("layout before");
        let outer_divider = before
            .dividers
            .iter()
            .filter(|divider| divider.orientation == DividerOrientation::Vertical)
            .max_by_key(|divider| divider.x)
            .copied()
            .expect("outer vertical divider");

        let down = comp.handle_mouse(&mouse_at(
            MouseEventKind::Down(MouseButton::Left),
            outer_divider.x as u16,
            outer_divider.y as u16,
        ));
        assert!(matches!(down, EventResult::Consumed));
        let drag = comp.handle_mouse(&mouse_at(
            MouseEventKind::Drag(MouseButton::Left),
            outer_divider.x.saturating_add(4) as u16,
            outer_divider.y as u16,
        ));
        assert!(matches!(drag, EventResult::Consumed));
        let up = comp.handle_mouse(&mouse_at(
            MouseEventKind::Up(MouseButton::Left),
            outer_divider.x.saturating_add(4) as u16,
            outer_divider.y as u16,
        ));
        assert!(matches!(up, EventResult::Consumed));

        let after = comp
            .window_layout_for_event_dims(80, 24)
            .expect("layout after");
        let outer_after = after
            .dividers
            .iter()
            .filter(|divider| divider.orientation == DividerOrientation::Vertical)
            .max_by_key(|divider| divider.x)
            .copied()
            .expect("outer vertical divider after");
        assert!(outer_after.x > outer_divider.x);
    }

    #[test]
    fn mouse_drag_keeps_state_after_hitting_resize_limit() {
        let mut comp = Compositor::new();
        comp.window_manager.split_focused(SplitAxis::Vertical, 2);
        comp.current = Surface::new(200, 24);

        let layout = comp
            .window_layout_for_event_dims(200, 24)
            .expect("layout before");
        let divider = layout
            .dividers
            .iter()
            .find(|divider| divider.orientation == DividerOrientation::Vertical)
            .copied()
            .expect("vertical divider");

        let first_drag_col = divider.x.saturating_add(60).min(190) as u16;
        let second_drag_col = first_drag_col.saturating_add(5);
        assert!(second_drag_col > first_drag_col);

        let down = comp.handle_mouse(&mouse_at(
            MouseEventKind::Down(MouseButton::Left),
            divider.x as u16,
            divider.y as u16,
        ));
        assert!(matches!(down, EventResult::Consumed));

        let drag_first = comp.handle_mouse(&mouse_at(
            MouseEventKind::Drag(MouseButton::Left),
            first_drag_col,
            divider.y as u16,
        ));
        assert!(matches!(drag_first, EventResult::Consumed));

        // Push again in the same direction; this can be a clamp/no-op error.
        let drag_at_limit = comp.handle_mouse(&mouse_at(
            MouseEventKind::Drag(MouseButton::Left),
            second_drag_col,
            divider.y as u16,
        ));
        assert!(matches!(drag_at_limit, EventResult::Consumed));

        // Reverse without releasing. Regression: this was ignored because drag
        // state was dropped on the previous no-op/error resize attempt.
        let drag_reverse = comp.handle_mouse(&mouse_at(
            MouseEventKind::Drag(MouseButton::Left),
            second_drag_col.saturating_sub(1),
            divider.y as u16,
        ));
        assert!(matches!(drag_reverse, EventResult::Consumed));

        let up = comp.handle_mouse(&mouse_at(
            MouseEventKind::Up(MouseButton::Left),
            second_drag_col.saturating_sub(1),
            divider.y as u16,
        ));
        assert!(matches!(up, EventResult::Consumed));
    }

    #[test]
    fn mouse_drag_non_divider_does_not_resize_windows() {
        let mut comp = Compositor::new();
        comp.window_manager.split_focused(SplitAxis::Vertical, 2);

        let before = comp
            .window_layout_for_event_dims(80, 24)
            .expect("layout before");
        let left = before
            .panes
            .iter()
            .find(|pane| pane.rect.x == 0)
            .expect("left pane");
        let start_col = left.rect.x as u16;
        let start_row = left.rect.y as u16;

        let down = comp.handle_mouse(&mouse_at(
            MouseEventKind::Down(MouseButton::Left),
            start_col,
            start_row,
        ));
        assert!(matches!(down, EventResult::Ignored));

        let drag = comp.handle_mouse(&mouse_at(
            MouseEventKind::Drag(MouseButton::Left),
            start_col.saturating_add(4),
            start_row,
        ));
        assert!(matches!(drag, EventResult::Ignored));

        let after = comp
            .window_layout_for_event_dims(80, 24)
            .expect("layout after");
        assert_eq!(after.panes, before.panes);
        assert_eq!(after.dividers, before.dividers);
    }

    #[test]
    fn mouse_drag_is_blocked_when_modal_overlay_active() {
        let mut comp = Compositor::new();
        comp.window_manager.split_focused(SplitAxis::Vertical, 2);
        comp.open_search_bar(0, 0, 0);

        let layout = comp.window_layout_for_event_dims(80, 24).expect("layout");
        let divider = layout
            .dividers
            .iter()
            .find(|divider| divider.orientation == DividerOrientation::Vertical)
            .copied()
            .expect("vertical divider");

        let down = comp.handle_mouse(&mouse_at(
            MouseEventKind::Down(MouseButton::Left),
            divider.x as u16,
            divider.y as u16,
        ));
        assert!(matches!(down, EventResult::Ignored));
        assert!(comp.mouse_drag.is_none());
    }

    #[test]
    fn mouse_up_clears_divider_drag_state() {
        let mut comp = Compositor::new();
        comp.window_manager.split_focused(SplitAxis::Vertical, 2);
        let layout = comp.window_layout_for_event_dims(80, 24).expect("layout");
        let divider = layout
            .dividers
            .iter()
            .find(|divider| divider.orientation == DividerOrientation::Vertical)
            .copied()
            .expect("vertical divider");

        let down = comp.handle_mouse(&mouse_at(
            MouseEventKind::Down(MouseButton::Left),
            divider.x as u16,
            divider.y as u16,
        ));
        assert!(matches!(down, EventResult::Consumed));
        assert!(comp.mouse_drag.is_some());

        let up = comp.handle_mouse(&mouse_at(
            MouseEventKind::Up(MouseButton::Left),
            divider.x as u16,
            divider.y as u16,
        ));
        assert!(matches!(up, EventResult::Consumed));
        assert!(comp.mouse_drag.is_none());
    }

    #[test]
    fn mouse_divider_window_pair_uses_clicked_divider_segment() {
        let layout = Layout {
            panes: vec![
                crate::ui::framework::window_manager::PaneLayout {
                    window_id: 1,
                    buffer_id: 1,
                    rect: PaneRect {
                        x: 0,
                        y: 0,
                        width: 10,
                        height: 5,
                    },
                },
                crate::ui::framework::window_manager::PaneLayout {
                    window_id: 2,
                    buffer_id: 2,
                    rect: PaneRect {
                        x: 11,
                        y: 0,
                        width: 9,
                        height: 5,
                    },
                },
                crate::ui::framework::window_manager::PaneLayout {
                    window_id: 3,
                    buffer_id: 3,
                    rect: PaneRect {
                        x: 0,
                        y: 6,
                        width: 10,
                        height: 5,
                    },
                },
                crate::ui::framework::window_manager::PaneLayout {
                    window_id: 4,
                    buffer_id: 4,
                    rect: PaneRect {
                        x: 11,
                        y: 6,
                        width: 9,
                        height: 5,
                    },
                },
            ],
            dividers: vec![
                Divider {
                    orientation: DividerOrientation::Vertical,
                    x: 10,
                    y: 0,
                    len: 5,
                },
                Divider {
                    orientation: DividerOrientation::Vertical,
                    x: 10,
                    y: 6,
                    len: 5,
                },
            ],
        };

        assert_eq!(
            Compositor::mouse_windows_for_divider(&layout, layout.dividers[0], 10, 1),
            Some((1, 2))
        );
        assert_eq!(
            Compositor::mouse_windows_for_divider(&layout, layout.dividers[1], 10, 7),
            Some((3, 4))
        );
    }

    #[test]
    fn search_bar_insert_text_japanese() {
        // Test that Japanese text (from IME paste events) is correctly inserted
        let mut bar = SearchBar {
            input: TextInput::default(),
            saved_cursor: 0,
            saved_scroll: 0,
            saved_horizontal_scroll: 0,
        };

        // Insert Japanese text (simulating IME composition result)
        bar.insert_text("ターミナル");
        assert_eq!(bar.input.text, "ターミナル");
        assert_eq!(bar.input.cursor, 5); // 5 characters

        // Insert more text at the end
        bar.insert_text("テスト");
        assert_eq!(bar.input.text, "ターミナルテスト");
        assert_eq!(bar.input.cursor, 8);

        // Insert at middle position
        bar.input.cursor = 5;
        bar.insert_text("の");
        assert_eq!(bar.input.text, "ターミナルのテスト");
        assert_eq!(bar.input.cursor, 6);
    }

    #[test]
    fn resize_emits_clear_screen() {
        use crate::config::Config;
        use crate::core::editor::Editor;
        use crate::input::chord::KeyState;
        use crate::syntax::theme::Theme;
        use crate::ui::framework::component::RenderContext;

        let editor = Editor::new();
        let config = Config::default();
        let theme = Theme::dark();
        let key_state = KeyState::Normal;

        let mut compositor = Compositor::new();

        // First render at 40x8
        let mut out1 = Vec::new();
        let ctx1 = RenderContext::new(
            40,
            8,
            &editor,
            &theme,
            &key_state,
            &config,
            std::path::Path::new("/tmp/gargo-test-root"),
            false,
            false,
        );
        compositor.render(&ctx1, &mut out1).expect("render frame 1");

        // Second render at 30x6 (simulating resize)
        let mut out2 = Vec::new();
        let ctx2 = RenderContext::new(
            30,
            6,
            &editor,
            &theme,
            &key_state,
            &config,
            std::path::Path::new("/tmp/gargo-test-root"),
            false,
            false,
        );
        compositor.render(&ctx2, &mut out2).expect("render frame 2");

        // The second render must contain \x1b[2J (clear all)
        let out2_str = String::from_utf8_lossy(&out2);
        assert!(
            out2_str.contains("\x1b[2J"),
            "Resize render must emit clear-screen escape sequence (\\x1b[2J)",
        );
    }

    #[test]
    fn same_size_render_does_not_emit_clear() {
        use crate::config::Config;
        use crate::core::editor::Editor;
        use crate::input::chord::KeyState;
        use crate::syntax::theme::Theme;
        use crate::ui::framework::component::RenderContext;

        let editor = Editor::new();
        let config = Config::default();
        let theme = Theme::dark();
        let key_state = KeyState::Normal;

        let mut compositor = Compositor::new();

        // First render at 40x8
        let mut out1 = Vec::new();
        let ctx1 = RenderContext::new(
            40,
            8,
            &editor,
            &theme,
            &key_state,
            &config,
            std::path::Path::new("/tmp/gargo-test-root"),
            false,
            false,
        );
        compositor.render(&ctx1, &mut out1).expect("render frame 1");

        // Second render at same size 40x8
        let mut out2 = Vec::new();
        let ctx2 = RenderContext::new(
            40,
            8,
            &editor,
            &theme,
            &key_state,
            &config,
            std::path::Path::new("/tmp/gargo-test-root"),
            false,
            false,
        );
        compositor.render(&ctx2, &mut out2).expect("render frame 2");

        // No resize => no clear
        let out2_str = String::from_utf8_lossy(&out2);
        assert!(
            !out2_str.contains("\x1b[2J"),
            "Same-size render must NOT emit clear-screen escape sequence",
        );
    }
}
