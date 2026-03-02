use super::*;

impl Compositor {
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

    pub fn search_bar_mut(&mut self) -> Option<&mut SearchBar> {
        self.search_bar.as_mut()
    }
}
