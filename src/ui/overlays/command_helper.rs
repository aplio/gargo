use crate::input::chord::KeyState;
use crate::syntax::theme::Theme;
use crate::ui::framework::cell::CellStyle;
use crate::ui::framework::surface::Surface;

pub struct CommandHelper {
    bindings: Vec<KeyBinding>,
    title: String,
}

struct KeyBinding {
    key: String,
    description: String,
}

impl CommandHelper {
    pub fn new(key_state: &KeyState) -> Self {
        let (title, bindings) = match key_state {
            KeyState::Space => (
                "SPC Commands".to_string(),
                vec![
                    KeyBinding {
                        key: "e".to_string(),
                        description: "Toggle explorer".to_string(),
                    },
                    KeyBinding {
                        key: "E".to_string(),
                        description: "Explorer popup".to_string(),
                    },
                    KeyBinding {
                        key: "f".to_string(),
                        description: "File picker".to_string(),
                    },
                    KeyBinding {
                        key: "b".to_string(),
                        description: "Buffer picker".to_string(),
                    },
                    KeyBinding {
                        key: "j".to_string(),
                        description: "Jumplist picker".to_string(),
                    },
                    KeyBinding {
                        key: "s".to_string(),
                        description: "Symbol picker".to_string(),
                    },
                    KeyBinding {
                        key: "p".to_string(),
                        description: "Command palette".to_string(),
                    },
                    KeyBinding {
                        key: "g".to_string(),
                        description: "Changed files sidebar".to_string(),
                    },
                    KeyBinding {
                        key: "G".to_string(),
                        description: "Git view".to_string(),
                    },
                    KeyBinding {
                        key: "l".to_string(),
                        description: "Commit log".to_string(),
                    },
                    KeyBinding {
                        key: "d".to_string(),
                        description: "Compare branch".to_string(),
                    },
                ],
            ),
            KeyState::SpaceWindow => (
                "SPC w Window Commands".to_string(),
                vec![
                    KeyBinding {
                        key: "v".to_string(),
                        description: "Vertical split".to_string(),
                    },
                    KeyBinding {
                        key: "s".to_string(),
                        description: "Horizontal split".to_string(),
                    },
                    KeyBinding {
                        key: "h/j/k/l or arrows".to_string(),
                        description: "Move focus".to_string(),
                    },
                    KeyBinding {
                        key: "w".to_string(),
                        description: "Focus next".to_string(),
                    },
                    KeyBinding {
                        key: "q".to_string(),
                        description: "Close window".to_string(),
                    },
                    KeyBinding {
                        key: "o".to_string(),
                        description: "Close other windows".to_string(),
                    },
                    KeyBinding {
                        key: "H/J/K/L or S-arrows".to_string(),
                        description: "Swap window".to_string(),
                    },
                ],
            ),
            KeyState::Goto => (
                "Goto Commands".to_string(),
                vec![
                    KeyBinding {
                        key: "g".to_string(),
                        description: "File start".to_string(),
                    },
                    KeyBinding {
                        key: "d".to_string(),
                        description: "Go definition".to_string(),
                    },
                    KeyBinding {
                        key: "r".to_string(),
                        description: "Go references".to_string(),
                    },
                    KeyBinding {
                        key: "e".to_string(),
                        description: "File end".to_string(),
                    },
                    KeyBinding {
                        key: "h".to_string(),
                        description: "Line start".to_string(),
                    },
                    KeyBinding {
                        key: "l".to_string(),
                        description: "Line end".to_string(),
                    },
                    KeyBinding {
                        key: "p".to_string(),
                        description: "Previous buffer".to_string(),
                    },
                    KeyBinding {
                        key: "n".to_string(),
                        description: "Next buffer".to_string(),
                    },
                ],
            ),
            KeyState::CtrlX => (
                "Ctrl-X Commands".to_string(),
                vec![
                    KeyBinding {
                        key: "C-s".to_string(),
                        description: "Save".to_string(),
                    },
                    KeyBinding {
                        key: "C-c".to_string(),
                        description: "Quit".to_string(),
                    },
                ],
            ),
            KeyState::MacroRecord => (
                "Record Macro".to_string(),
                vec![KeyBinding {
                    key: "a-z".to_string(),
                    description: "Select register".to_string(),
                }],
            ),
            KeyState::MacroPlay => (
                "Play Macro".to_string(),
                vec![KeyBinding {
                    key: "a-z".to_string(),
                    description: "Select register".to_string(),
                }],
            ),
            KeyState::Normal => {
                // Should not happen
                ("".to_string(), vec![])
            }
        };

        CommandHelper { bindings, title }
    }

    pub fn render_overlay(&self, surface: &mut Surface, cols: usize, rows: usize, _theme: &Theme) {
        // Don't show on small screens
        if cols < 60 || self.bindings.is_empty() {
            return;
        }

        let width = (cols * 40) / 100;
        let height = self.bindings.len() + 3; // title + borders + content

        // Don't render if height exceeds screen
        if height > rows {
            return;
        }

        let x = cols.saturating_sub(width);
        let y = (rows.saturating_sub(height)) / 2; // centered vertically

        let default_style = CellStyle::default();
        let dim_style = CellStyle {
            dim: true,
            ..CellStyle::default()
        };

        // Draw top border
        surface.put_str(x, y, "┌", &dim_style);
        for i in 1..width - 1 {
            surface.put_str(x + i, y, "─", &dim_style);
        }
        surface.put_str(x + width - 1, y, "┐", &dim_style);

        // Draw title line
        surface.put_str(x, y + 1, "│", &dim_style);
        let title_with_padding = format!(" {} ", self.title);
        let title_len = title_with_padding.len().min(width - 2);
        surface.put_str(
            x + 1,
            y + 1,
            &title_with_padding[..title_len],
            &default_style,
        );
        // Fill rest of title line
        for i in (title_len + 1)..(width - 1) {
            surface.put_str(x + i, y + 1, " ", &default_style);
        }
        surface.put_str(x + width - 1, y + 1, "│", &dim_style);

        // Draw keybindings
        for (idx, binding) in self.bindings.iter().enumerate() {
            let line_y = y + 2 + idx;
            surface.put_str(x, line_y, "│", &dim_style);

            let content = format!(" {} - {}", binding.key, binding.description);
            let content_len = content.len().min(width - 2);
            surface.put_str(x + 1, line_y, &content[..content_len], &default_style);
            // Fill rest of line
            for i in (content_len + 1)..(width - 1) {
                surface.put_str(x + i, line_y, " ", &default_style);
            }
            surface.put_str(x + width - 1, line_y, "│", &dim_style);
        }

        // Draw bottom border
        let bottom_y = y + height - 1;
        surface.put_str(x, bottom_y, "└", &dim_style);
        for i in 1..width - 1 {
            surface.put_str(x + i, bottom_y, "─", &dim_style);
        }
        surface.put_str(x + width - 1, bottom_y, "┘", &dim_style);
    }
}
