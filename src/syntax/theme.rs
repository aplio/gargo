use std::collections::HashMap;

use crate::config::{Config, ThemeCaptureConfig, ThemeConfig, ThemeUiConfig};
use crossterm::style::Color;

#[derive(Clone, Debug, Default)]
pub struct Style {
    pub fg: Option<Color>,
    pub bold: bool,
    pub italic: bool,
}

pub struct Theme {
    mappings: HashMap<String, Style>,
    markdown_link_hover_bg: Color,
    markdown_link_hover_selected_bg: Color,
}

impl Theme {
    pub fn ansi_dark() -> Self {
        Self::from_entries(vec![
            (
                "keyword",
                Style {
                    fg: Some(Color::Magenta),
                    bold: true,
                    italic: false,
                },
            ),
            (
                "keyword.control",
                Style {
                    fg: Some(Color::Magenta),
                    bold: true,
                    italic: false,
                },
            ),
            (
                "keyword.operator",
                Style {
                    fg: Some(Color::Magenta),
                    bold: false,
                    italic: false,
                },
            ),
            (
                "keyword.function",
                Style {
                    fg: Some(Color::Magenta),
                    bold: true,
                    italic: false,
                },
            ),
            (
                "keyword.return",
                Style {
                    fg: Some(Color::Magenta),
                    bold: true,
                    italic: false,
                },
            ),
            (
                "keyword.import",
                Style {
                    fg: Some(Color::Magenta),
                    bold: true,
                    italic: false,
                },
            ),
            (
                "operator",
                Style {
                    fg: Some(Color::White),
                    bold: false,
                    italic: false,
                },
            ),
            (
                "string",
                Style {
                    fg: Some(Color::Green),
                    bold: false,
                    italic: false,
                },
            ),
            (
                "string.special",
                Style {
                    fg: Some(Color::Green),
                    bold: false,
                    italic: false,
                },
            ),
            (
                "comment",
                Style {
                    fg: Some(Color::DarkGrey),
                    bold: false,
                    italic: true,
                },
            ),
            (
                "comment.line",
                Style {
                    fg: Some(Color::DarkGrey),
                    bold: false,
                    italic: true,
                },
            ),
            (
                "comment.block",
                Style {
                    fg: Some(Color::DarkGrey),
                    bold: false,
                    italic: true,
                },
            ),
            (
                "function",
                Style {
                    fg: Some(Color::Blue),
                    bold: false,
                    italic: false,
                },
            ),
            (
                "function.call",
                Style {
                    fg: Some(Color::Blue),
                    bold: false,
                    italic: false,
                },
            ),
            (
                "function.method",
                Style {
                    fg: Some(Color::Blue),
                    bold: false,
                    italic: false,
                },
            ),
            (
                "function.macro",
                Style {
                    fg: Some(Color::Blue),
                    bold: true,
                    italic: false,
                },
            ),
            (
                "function.builtin",
                Style {
                    fg: Some(Color::Blue),
                    bold: false,
                    italic: false,
                },
            ),
            (
                "type",
                Style {
                    fg: Some(Color::Yellow),
                    bold: false,
                    italic: false,
                },
            ),
            (
                "type.builtin",
                Style {
                    fg: Some(Color::Yellow),
                    bold: false,
                    italic: false,
                },
            ),
            (
                "constructor",
                Style {
                    fg: Some(Color::Yellow),
                    bold: false,
                    italic: false,
                },
            ),
            (
                "constant",
                Style {
                    fg: Some(Color::Cyan),
                    bold: false,
                    italic: false,
                },
            ),
            (
                "constant.builtin",
                Style {
                    fg: Some(Color::Cyan),
                    bold: false,
                    italic: false,
                },
            ),
            (
                "number",
                Style {
                    fg: Some(Color::Cyan),
                    bold: false,
                    italic: false,
                },
            ),
            (
                "float",
                Style {
                    fg: Some(Color::Cyan),
                    bold: false,
                    italic: false,
                },
            ),
            (
                "boolean",
                Style {
                    fg: Some(Color::Cyan),
                    bold: false,
                    italic: false,
                },
            ),
            (
                "variable",
                Style {
                    fg: Some(Color::White),
                    bold: false,
                    italic: false,
                },
            ),
            (
                "variable.builtin",
                Style {
                    fg: Some(Color::Red),
                    bold: false,
                    italic: false,
                },
            ),
            (
                "variable.parameter",
                Style {
                    fg: Some(Color::White),
                    bold: false,
                    italic: true,
                },
            ),
            (
                "property",
                Style {
                    fg: Some(Color::White),
                    bold: false,
                    italic: false,
                },
            ),
            (
                "attribute",
                Style {
                    fg: Some(Color::Cyan),
                    bold: false,
                    italic: false,
                },
            ),
            (
                "label",
                Style {
                    fg: Some(Color::Cyan),
                    bold: false,
                    italic: false,
                },
            ),
            (
                "punctuation",
                Style {
                    fg: Some(Color::White),
                    bold: false,
                    italic: false,
                },
            ),
            (
                "punctuation.bracket",
                Style {
                    fg: Some(Color::White),
                    bold: false,
                    italic: false,
                },
            ),
            (
                "punctuation.delimiter",
                Style {
                    fg: Some(Color::White),
                    bold: false,
                    italic: false,
                },
            ),
            (
                "escape",
                Style {
                    fg: Some(Color::Cyan),
                    bold: false,
                    italic: false,
                },
            ),
            (
                "embedded",
                Style {
                    fg: Some(Color::White),
                    bold: false,
                    italic: false,
                },
            ),
            (
                "tag",
                Style {
                    fg: Some(Color::Red),
                    bold: false,
                    italic: false,
                },
            ),
            (
                "heading",
                Style {
                    fg: Some(Color::Blue),
                    bold: true,
                    italic: false,
                },
            ),
            (
                "title",
                Style {
                    fg: Some(Color::Blue),
                    bold: true,
                    italic: false,
                },
            ),
            (
                "link",
                Style {
                    fg: Some(Color::Cyan),
                    bold: false,
                    italic: false,
                },
            ),
            (
                "emphasis",
                Style {
                    fg: None,
                    bold: false,
                    italic: true,
                },
            ),
            (
                "strong",
                Style {
                    fg: None,
                    bold: true,
                    italic: false,
                },
            ),
            (
                "namespace",
                Style {
                    fg: Some(Color::Yellow),
                    bold: false,
                    italic: false,
                },
            ),
            (
                "module",
                Style {
                    fg: Some(Color::Yellow),
                    bold: false,
                    italic: false,
                },
            ),
            // text.* captures (used by Markdown, etc.)
            (
                "text.title",
                Style {
                    fg: Some(Color::Blue),
                    bold: true,
                    italic: false,
                },
            ),
            (
                "text.literal",
                Style {
                    fg: Some(Color::Green),
                    bold: false,
                    italic: false,
                },
            ),
            (
                "text.uri",
                Style {
                    fg: Some(Color::Cyan),
                    bold: false,
                    italic: false,
                },
            ),
            (
                "text.reference",
                Style {
                    fg: Some(Color::Cyan),
                    bold: false,
                    italic: false,
                },
            ),
            (
                "text.emphasis",
                Style {
                    fg: None,
                    bold: false,
                    italic: true,
                },
            ),
            (
                "text.strong",
                Style {
                    fg: None,
                    bold: true,
                    italic: false,
                },
            ),
            (
                "punctuation.special",
                Style {
                    fg: Some(Color::DarkGrey),
                    bold: true,
                    italic: false,
                },
            ),
            (
                "diff.header",
                Style {
                    fg: Some(Color::Blue),
                    bold: true,
                    italic: false,
                },
            ),
            (
                "diff.section",
                Style {
                    fg: Some(Color::Magenta),
                    bold: true,
                    italic: false,
                },
            ),
            (
                "diff.meta",
                Style {
                    fg: Some(Color::DarkGrey),
                    bold: false,
                    italic: false,
                },
            ),
            (
                "diff.help",
                Style {
                    fg: Some(Color::Cyan),
                    bold: false,
                    italic: false,
                },
            ),
            (
                "diff.hunk",
                Style {
                    fg: Some(Color::Yellow),
                    bold: true,
                    italic: false,
                },
            ),
            (
                "diff.plus",
                Style {
                    fg: Some(Color::Green),
                    bold: false,
                    italic: false,
                },
            ),
            (
                "diff.minus",
                Style {
                    fg: Some(Color::Red),
                    bold: false,
                    italic: false,
                },
            ),
            (
                "diff.context",
                Style {
                    fg: Some(Color::White),
                    bold: false,
                    italic: false,
                },
            ),
            (
                "diff.plus.gutter",
                Style {
                    fg: Some(Color::Green),
                    bold: false,
                    italic: false,
                },
            ),
            (
                "diff.delta.gutter",
                Style {
                    fg: Some(Color::Yellow),
                    bold: false,
                    italic: false,
                },
            ),
            (
                "diff.minus.gutter",
                Style {
                    fg: Some(Color::Red),
                    bold: false,
                    italic: false,
                },
            ),
        ])
    }

    pub fn ansi_light() -> Self {
        let mut theme = Self::ansi_dark();
        for style in theme.mappings.values_mut() {
            style.fg = style.fg.map(light_variant);
        }
        theme
    }

    pub fn dark() -> Self {
        Self::ansi_dark()
    }

    pub fn load() -> Self {
        let config = Config::load();
        Self::from_config(&config.theme)
    }

    pub fn from_config(theme_config: &ThemeConfig) -> Self {
        let mut theme = match normalize_preset_name(&theme_config.preset) {
            "ansi_dark" => Self::ansi_dark(),
            "ansi_light" => Self::ansi_light(),
            _ => Self::ansi_dark(),
        };
        for (capture, override_style) in &theme_config.captures {
            theme.apply_capture_override(capture, override_style);
        }
        theme.apply_ui_overrides(&theme_config.ui);
        theme
    }

    fn from_entries(entries: Vec<(&'static str, Style)>) -> Self {
        let mappings = entries
            .into_iter()
            .map(|(name, style)| (name.to_string(), style))
            .collect();
        Self {
            mappings,
            markdown_link_hover_bg: Color::DarkGrey,
            markdown_link_hover_selected_bg: Color::Grey,
        }
    }

    fn apply_capture_override(&mut self, capture: &str, override_style: &ThemeCaptureConfig) {
        let mut style = self.mappings.get(capture).cloned().unwrap_or_default();
        if let Some(color_text) = &override_style.fg
            && let Some(color) = parse_color(color_text)
        {
            style.fg = Some(color);
        }
        if let Some(bold) = override_style.bold {
            style.bold = bold;
        }
        if let Some(italic) = override_style.italic {
            style.italic = italic;
        }
        self.mappings.insert(capture.to_string(), style);
    }

    fn apply_ui_overrides(&mut self, ui: &ThemeUiConfig) {
        if let Some(color_text) = &ui.markdown_link_hover_bg
            && let Some(color) = parse_color(color_text)
        {
            self.markdown_link_hover_bg = color;
        }
        if let Some(color_text) = &ui.markdown_link_hover_selected_bg
            && let Some(color) = parse_color(color_text)
        {
            self.markdown_link_hover_selected_bg = color;
        }
    }

    pub fn markdown_link_hover_bg(&self) -> Color {
        self.markdown_link_hover_bg
    }

    pub fn markdown_link_hover_selected_bg(&self) -> Color {
        self.markdown_link_hover_selected_bg
    }

    /// Look up style for a capture name, with hierarchical fallback.
    /// e.g. "function.method" → try "function.method" → "function"
    pub fn style_for_capture(&self, capture_name: &str) -> Option<&Style> {
        // Try exact match first
        if let Some(style) = self.find_mapping(capture_name) {
            return Some(style);
        }
        // Hierarchical fallback: strip last segment
        let mut name = capture_name;
        while let Some(dot_pos) = name.rfind('.') {
            name = &name[..dot_pos];
            if let Some(style) = self.find_mapping(name) {
                return Some(style);
            }
        }
        None
    }

    fn find_mapping(&self, name: &str) -> Option<&Style> {
        self.mappings.get(name)
    }
}

fn normalize_preset_name(name: &str) -> &'static str {
    match name.trim().to_ascii_lowercase().as_str() {
        "dark" | "ansi_dark" => "ansi_dark",
        "light" | "ansi_light" => "ansi_light",
        _ => "",
    }
}

fn parse_color(input: &str) -> Option<Color> {
    if let Some(hex) = input.strip_prefix('#')
        && hex.len() == 6
    {
        let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
        let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
        let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
        return Some(Color::Rgb { r, g, b });
    }

    match input.trim().to_ascii_lowercase().replace('-', "_").as_str() {
        "black" => Some(Color::Black),
        "dark_grey" => Some(Color::DarkGrey),
        "grey" => Some(Color::Grey),
        "white" => Some(Color::White),
        "red" => Some(Color::Red),
        "dark_red" => Some(Color::DarkRed),
        "green" => Some(Color::Green),
        "dark_green" => Some(Color::DarkGreen),
        "yellow" => Some(Color::Yellow),
        "dark_yellow" => Some(Color::DarkYellow),
        "blue" => Some(Color::Blue),
        "dark_blue" => Some(Color::DarkBlue),
        "magenta" => Some(Color::Magenta),
        "dark_magenta" => Some(Color::DarkMagenta),
        "cyan" => Some(Color::Cyan),
        "dark_cyan" => Some(Color::DarkCyan),
        _ => None,
    }
}

fn light_variant(color: Color) -> Color {
    match color {
        Color::White => Color::Black,
        Color::Magenta => Color::DarkMagenta,
        Color::Green => Color::DarkGreen,
        Color::Blue => Color::DarkBlue,
        Color::Yellow => Color::DarkYellow,
        Color::Cyan => Color::DarkCyan,
        Color::Red => Color::DarkRed,
        Color::DarkGrey => Color::Grey,
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(())).lock().unwrap()
    }

    #[test]
    fn exact_match() {
        let theme = Theme::dark();
        let style = theme.style_for_capture("keyword").unwrap();
        assert_eq!(style.fg, Some(Color::Magenta));
    }

    #[test]
    fn hierarchical_fallback() {
        let theme = Theme::dark();
        // "function.method.call" should fallback to "function.method" or "function"
        let style = theme.style_for_capture("function.method.call").unwrap();
        assert_eq!(style.fg, Some(Color::Blue));
    }

    #[test]
    fn unknown_capture() {
        let theme = Theme::dark();
        assert!(theme.style_for_capture("unknown_capture_xyz").is_none());
    }

    #[test]
    fn comment_is_italic() {
        let theme = Theme::dark();
        let style = theme.style_for_capture("comment").unwrap();
        assert!(style.italic);
    }

    #[test]
    fn ansi_light_adjusts_dark_preset_colors() {
        let theme = Theme::ansi_light();
        let keyword = theme.style_for_capture("keyword").unwrap();
        assert_eq!(keyword.fg, Some(Color::DarkMagenta));
        let operator = theme.style_for_capture("operator").unwrap();
        assert_eq!(operator.fg, Some(Color::Black));
    }

    #[test]
    fn diff_captures_have_default_colors() {
        let theme = Theme::dark();
        assert_eq!(
            theme
                .style_for_capture("diff.plus")
                .and_then(|style| style.fg),
            Some(Color::Green)
        );
        assert_eq!(
            theme
                .style_for_capture("diff.minus")
                .and_then(|style| style.fg),
            Some(Color::Red)
        );
        assert_eq!(
            theme
                .style_for_capture("diff.hunk")
                .and_then(|style| style.fg),
            Some(Color::Yellow)
        );
    }

    #[test]
    fn from_config_applies_preset_and_capture_overrides() {
        let cfg: Config = toml::from_str(
            r##"
[theme]
preset = "ansi_light"

[theme.captures]
"keyword" = { fg = "#112233", bold = false }
"comment" = { fg = "dark_grey", italic = false }
"custom.capture" = { fg = "red", bold = true, italic = true }
"bad.color" = { fg = "not_a_color" }

[theme.ui]
markdown_link_hover_bg = "#121314"
markdown_link_hover_selected_bg = "grey"
"##,
        )
        .unwrap();

        let theme = Theme::from_config(&cfg.theme);
        let keyword = theme.style_for_capture("keyword").unwrap();
        assert_eq!(
            keyword.fg,
            Some(Color::Rgb {
                r: 0x11,
                g: 0x22,
                b: 0x33
            })
        );
        assert!(!keyword.bold);

        let comment = theme.style_for_capture("comment").unwrap();
        assert_eq!(comment.fg, Some(Color::DarkGrey));
        assert!(!comment.italic);

        let custom = theme.style_for_capture("custom.capture").unwrap();
        assert_eq!(custom.fg, Some(Color::Red));
        assert!(custom.bold);
        assert!(custom.italic);

        assert!(theme.style_for_capture("bad.color").is_some());
        assert_eq!(theme.style_for_capture("bad.color").unwrap().fg, None);
        assert_eq!(
            theme.markdown_link_hover_bg(),
            Color::Rgb {
                r: 0x12,
                g: 0x13,
                b: 0x14
            }
        );
        assert_eq!(theme.markdown_link_hover_selected_bg(), Color::Grey);
    }

    #[test]
    fn load_uses_theme_section_from_config_toml() {
        let _guard = env_lock();
        let tmp = tempfile::tempdir().unwrap();
        let gargo_dir = tmp.path().join("gargo");
        std::fs::create_dir_all(&gargo_dir).unwrap();
        std::fs::write(
            gargo_dir.join("config.toml"),
            r#"
[theme]
preset = "ansi_dark"

[theme.captures]
"keyword" = { fg = "dark_blue" }
"#,
        )
        .unwrap();

        unsafe {
            std::env::set_var("XDG_CONFIG_HOME", tmp.path());
        }
        let theme = Theme::load();
        unsafe {
            std::env::remove_var("XDG_CONFIG_HOME");
        }

        let keyword = theme.style_for_capture("keyword").unwrap();
        assert_eq!(keyword.fg, Some(Color::DarkBlue));
    }

    #[test]
    fn load_ignores_legacy_theme_toml_even_if_present() {
        let _guard = env_lock();
        let tmp = tempfile::tempdir().unwrap();
        let gargo_dir = tmp.path().join("gargo");
        std::fs::create_dir_all(&gargo_dir).unwrap();
        std::fs::write(
            gargo_dir.join("config.toml"),
            r#"
[theme]
preset = "ansi_dark"

[theme.captures]
"keyword" = { fg = "dark_blue" }
"#,
        )
        .unwrap();
        std::fs::write(
            gargo_dir.join("theme.toml"),
            r#"
preset = "ansi_dark"
[captures]
"keyword" = { fg = "red" }
"#,
        )
        .unwrap();

        unsafe {
            std::env::set_var("XDG_CONFIG_HOME", tmp.path());
        }
        let theme = Theme::load();
        unsafe {
            std::env::remove_var("XDG_CONFIG_HOME");
        }

        let keyword = theme.style_for_capture("keyword").unwrap();
        assert_eq!(keyword.fg, Some(Color::DarkBlue));
    }

    #[test]
    fn from_config_falls_back_to_default_for_invalid_preset() {
        let cfg: Config = toml::from_str(
            r#"
[theme]
preset = "unknown"

[theme.captures]
"keyword" = { fg = "white" }
"#,
        )
        .unwrap();
        let theme = Theme::from_config(&cfg.theme);
        let keyword = theme.style_for_capture("keyword").unwrap();
        assert_eq!(keyword.fg, Some(Color::White));
    }

    #[test]
    fn from_config_keeps_hover_defaults_for_invalid_ui_colors() {
        let cfg: Config = toml::from_str(
            r#"
[theme]
preset = "ansi_dark"

[theme.ui]
markdown_link_hover_bg = "not-a-color"
markdown_link_hover_selected_bg = "also-not-a-color"
"#,
        )
        .unwrap();
        let theme = Theme::from_config(&cfg.theme);
        assert_eq!(theme.markdown_link_hover_bg(), Color::DarkGrey);
        assert_eq!(theme.markdown_link_hover_selected_bg(), Color::Grey);
    }

    #[test]
    fn load_falls_back_to_default_for_invalid_config_toml() {
        let _guard = env_lock();
        let tmp = tempfile::tempdir().unwrap();
        let gargo_dir = tmp.path().join("gargo");
        std::fs::create_dir_all(&gargo_dir).unwrap();
        std::fs::write(gargo_dir.join("config.toml"), "not valid = ").unwrap();

        unsafe {
            std::env::set_var("XDG_CONFIG_HOME", tmp.path());
        }
        let fallback = Theme::load();
        unsafe {
            std::env::remove_var("XDG_CONFIG_HOME");
        }

        assert_eq!(
            fallback.style_for_capture("keyword").unwrap().fg,
            Some(Color::Magenta)
        );
    }
}
