use super::*;

impl Compositor {
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
}
