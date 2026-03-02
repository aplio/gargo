use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::syntax::theme::Theme;
use crate::ui::framework::cell::CellStyle;
use crate::ui::framework::surface::Surface;
use crate::ui::text::{display_width, truncate_to_width};

const MAX_VISIBLE_ITEMS: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HoverKeyResult {
    Ignored,
    Consumed,
    Close,
    Apply(String),
}

pub struct MarkdownLinkHover {
    candidates: Vec<String>,
    selected: usize,
}

impl MarkdownLinkHover {
    pub fn new(candidates: Vec<String>) -> Self {
        Self {
            candidates,
            selected: 0,
        }
    }

    pub fn set_candidates(&mut self, candidates: Vec<String>) {
        self.candidates = candidates;
        if self.selected >= self.candidates.len() {
            self.selected = self.candidates.len().saturating_sub(1);
        }
    }

    pub fn is_empty(&self) -> bool {
        self.candidates.is_empty()
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> HoverKeyResult {
        if self.candidates.is_empty() {
            return HoverKeyResult::Ignored;
        }

        if key.modifiers.contains(KeyModifiers::CONTROL) {
            return match key.code {
                KeyCode::Char('n') => {
                    self.select_next();
                    HoverKeyResult::Consumed
                }
                KeyCode::Char('p') => {
                    self.select_prev();
                    HoverKeyResult::Consumed
                }
                _ => HoverKeyResult::Ignored,
            };
        }

        match key.code {
            KeyCode::Tab => {
                if key.modifiers.contains(KeyModifiers::SHIFT) {
                    self.select_prev();
                } else {
                    self.select_next();
                }
                HoverKeyResult::Consumed
            }
            KeyCode::BackTab => {
                self.select_prev();
                HoverKeyResult::Consumed
            }
            KeyCode::Up => {
                self.select_prev();
                HoverKeyResult::Consumed
            }
            KeyCode::Down => {
                self.select_next();
                HoverKeyResult::Consumed
            }
            KeyCode::Enter => self
                .selected_candidate()
                .map(HoverKeyResult::Apply)
                .unwrap_or(HoverKeyResult::Consumed),
            KeyCode::Esc => HoverKeyResult::Close,
            _ => HoverKeyResult::Ignored,
        }
    }

    pub fn render_overlay(
        &self,
        surface: &mut Surface,
        cursor_x: usize,
        cursor_y: usize,
        theme: &Theme,
    ) {
        if self.candidates.is_empty() || surface.width == 0 || surface.height == 0 {
            return;
        }

        let visible_count = self.candidates.len().min(MAX_VISIBLE_ITEMS);
        let max_label_width = self
            .candidates
            .iter()
            .take(visible_count)
            .map(|c| display_width(c))
            .max()
            .unwrap_or(1);
        let list_width = max_label_width.max(1).min(surface.width);
        let list_height = visible_count.min(surface.height);
        if list_height == 0 {
            return;
        }

        let x = cursor_x.min(surface.width.saturating_sub(list_width));
        let y = if cursor_y + list_height < surface.height {
            cursor_y + 1
        } else {
            cursor_y.saturating_sub(list_height)
        };

        let render_rows = list_height;
        let mut start = 0usize;
        if self.selected >= render_rows {
            start = self.selected + 1 - render_rows;
        }

        let default_style = CellStyle {
            bg: Some(theme.markdown_link_hover_bg()),
            ..CellStyle::default()
        };
        let selected_style = CellStyle {
            bg: Some(theme.markdown_link_hover_selected_bg()),
            ..CellStyle::default()
        };

        for row in 0..render_rows {
            let item_idx = start + row;
            if item_idx >= self.candidates.len() {
                break;
            }
            let style = if item_idx == self.selected {
                &selected_style
            } else {
                &default_style
            };
            let inner_y = y + row;
            let inner_x = x;
            let inner_w = list_width;
            surface.fill_region(inner_x, inner_y, inner_w, ' ', style);

            let (label, _) = truncate_to_width(&self.candidates[item_idx], inner_w);
            surface.put_str(inner_x, inner_y, label, style);
        }
    }

    fn select_next(&mut self) {
        if self.candidates.is_empty() {
            return;
        }
        self.selected = (self.selected + 1) % self.candidates.len();
    }

    fn select_prev(&mut self) {
        if self.candidates.is_empty() {
            return;
        }
        self.selected = if self.selected == 0 {
            self.candidates.len() - 1
        } else {
            self.selected - 1
        };
    }

    fn selected_candidate(&self) -> Option<String> {
        self.candidates.get(self.selected).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::ui::framework::surface::Surface;
    use crossterm::style::Color;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn ctrl(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    #[test]
    fn ctrl_navigation_cycles_candidates() {
        let mut hover = MarkdownLinkHover::new(vec!["a".to_string(), "b".to_string()]);

        assert_eq!(hover.handle_key(ctrl('n')), HoverKeyResult::Consumed);
        assert_eq!(hover.selected_candidate().as_deref(), Some("b"));

        assert_eq!(hover.handle_key(ctrl('p')), HoverKeyResult::Consumed);
        assert_eq!(hover.selected_candidate().as_deref(), Some("a"));
    }

    #[test]
    fn tab_and_backtab_navigate_candidates() {
        let mut hover = MarkdownLinkHover::new(vec!["a".to_string(), "b".to_string()]);

        assert_eq!(
            hover.handle_key(key(KeyCode::Tab)),
            HoverKeyResult::Consumed
        );
        assert_eq!(hover.selected_candidate().as_deref(), Some("b"));

        assert_eq!(
            hover.handle_key(key(KeyCode::BackTab)),
            HoverKeyResult::Consumed
        );
        assert_eq!(hover.selected_candidate().as_deref(), Some("a"));
    }

    #[test]
    fn up_and_down_navigate_candidates() {
        let mut hover =
            MarkdownLinkHover::new(vec!["a".to_string(), "b".to_string(), "c".to_string()]);

        assert_eq!(
            hover.handle_key(key(KeyCode::Down)),
            HoverKeyResult::Consumed
        );
        assert_eq!(hover.selected_candidate().as_deref(), Some("b"));

        assert_eq!(hover.handle_key(key(KeyCode::Up)), HoverKeyResult::Consumed);
        assert_eq!(hover.selected_candidate().as_deref(), Some("a"));

        assert_eq!(hover.handle_key(key(KeyCode::Up)), HoverKeyResult::Consumed);
        assert_eq!(hover.selected_candidate().as_deref(), Some("c"));
    }

    #[test]
    fn enter_applies_selected_candidate() {
        let mut hover = MarkdownLinkHover::new(vec!["a.md".to_string(), "b.md".to_string()]);
        let _ = hover.handle_key(key(KeyCode::Tab));

        assert_eq!(
            hover.handle_key(key(KeyCode::Enter)),
            HoverKeyResult::Apply("b.md".to_string())
        );
    }

    #[test]
    fn esc_closes_hover() {
        let mut hover = MarkdownLinkHover::new(vec!["a".to_string()]);
        assert_eq!(hover.handle_key(key(KeyCode::Esc)), HoverKeyResult::Close);
    }

    #[test]
    fn non_control_keys_fall_through() {
        let mut hover = MarkdownLinkHover::new(vec!["a".to_string()]);
        assert_eq!(
            hover.handle_key(key(KeyCode::Char('x'))),
            HoverKeyResult::Ignored
        );
    }

    #[test]
    fn overlay_rows_use_distinct_background_colors() {
        let hover = MarkdownLinkHover::new(vec!["alpha".to_string(), "beta".to_string()]);
        let mut surface = Surface::new(20, 6);
        let theme = Theme::dark();

        hover.render_overlay(&mut surface, 0, 0, &theme);

        assert_eq!(surface.get(0, 0).style.bg, None);
        assert_eq!(surface.get(0, 1).style.bg, Some(Color::Grey));
        assert_eq!(surface.get(0, 2).style.bg, Some(Color::DarkGrey));
    }

    #[test]
    fn overlay_uses_theme_ui_background_colors() {
        let cfg: Config = toml::from_str(
            r##"
[theme]
preset = "ansi_dark"

[theme.ui]
markdown_link_hover_bg = "#112233"
markdown_link_hover_selected_bg = "#445566"
"##,
        )
        .unwrap();
        let theme = Theme::from_config(&cfg.theme);
        let hover = MarkdownLinkHover::new(vec!["alpha".to_string(), "beta".to_string()]);
        let mut surface = Surface::new(20, 6);

        hover.render_overlay(&mut surface, 0, 0, &theme);

        assert_eq!(
            surface.get(0, 1).style.bg,
            Some(Color::Rgb {
                r: 0x44,
                g: 0x55,
                b: 0x66
            })
        );
        assert_eq!(
            surface.get(0, 2).style.bg,
            Some(Color::Rgb {
                r: 0x11,
                g: 0x22,
                b: 0x33
            })
        );
    }
}
