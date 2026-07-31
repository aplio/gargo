use std::collections::HashMap;

use crate::config::{Config, ThemeCaptureConfig, ThemeConfig, ThemeUiConfig};
use crate::syntax::ui_colors::UiColors;
use crossterm::style::Color;

#[derive(Clone, Debug, Default)]
pub struct Style {
    pub fg: Option<Color>,
    pub bold: bool,
    pub italic: bool,
}

pub struct Theme {
    mappings: HashMap<String, Style>,
    /// Colors for everything that is not a syntax token. Always concrete RGB,
    /// including under the `ansi_*` presets: the terminal's palette should
    /// decide how *code* looks, not whether the sidebar is readable.
    pub ui: UiColors,
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
        theme.ui = UiColors::light();
        theme
    }

    /// Same capture set as [`Theme::ansi_dark`], resolved through a fixed
    /// palette instead of ANSI names. Built by remapping rather than by
    /// listing all ~60 captures again, so a capture added to the base table
    /// cannot go missing from a preset.
    pub fn from_palette(palette: &SyntaxPalette, ui: UiColors) -> Self {
        let mut theme = Self::ansi_dark();
        for style in theme.mappings.values_mut() {
            style.fg = style.fg.map(|color| palette.resolve(color));
        }
        theme.markdown_link_hover_bg = ui.selected_bg;
        theme.markdown_link_hover_selected_bg = ui.accent;
        theme.ui = ui;
        theme
    }

    pub fn gargo_dark() -> Self {
        Self::from_palette(&PALETTE_DARK, UiColors::dark())
    }

    pub fn gargo_light() -> Self {
        Self::from_palette(&PALETTE_LIGHT, UiColors::light())
    }

    pub fn gargo_dim() -> Self {
        Self::from_palette(&PALETTE_DIM, UiColors::dim())
    }

    pub fn gargo_contrast() -> Self {
        Self::from_palette(&PALETTE_CONTRAST, UiColors::contrast())
    }

    pub fn gargo_sepia() -> Self {
        Self::from_palette(&PALETTE_SEPIA, UiColors::sepia())
    }

    pub fn dark() -> Self {
        Self::ansi_dark()
    }

    pub fn load() -> Self {
        let config = Config::load();
        Self::from_config(&config.theme)
    }

    pub fn from_config(theme_config: &ThemeConfig) -> Self {
        let mut theme = find_preset(&theme_config.preset)
            .map(|preset| preset.build())
            .unwrap_or_else(Self::ansi_dark);
        theme.apply_config_overrides(theme_config);
        theme
    }

    /// Layer a config's capture and UI overrides onto an already-built preset.
    /// Split out so live preview can swap the preset while keeping whatever
    /// the user has customised on top of it.
    pub fn apply_config_overrides(&mut self, theme_config: &ThemeConfig) {
        for (capture, override_style) in &theme_config.captures {
            self.apply_capture_override(capture, override_style);
        }
        self.apply_ui_overrides(&theme_config.ui);
    }

    fn from_entries(entries: Vec<(&'static str, Style)>) -> Self {
        let mappings = entries
            .into_iter()
            .map(|(name, style)| (name.to_string(), style))
            .collect();
        Self {
            mappings,
            ui: UiColors::dark(),
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

        macro_rules! override_role {
            ($($role:ident),+ $(,)?) => {
                $(
                    if let Some(color_text) = &ui.$role
                        && let Some(color) = parse_color(color_text)
                    {
                        self.ui.$role = color;
                    }
                )+
            };
        }
        override_role!(
            bg,
            panel_bg,
            text,
            dim,
            faint,
            accent,
            selected_bg,
            selected_fg,
            folder,
            dirty,
            status_bg,
            status_fg,
            mode_fg,
            mode_normal,
            mode_insert,
            mode_visual,
            error,
            warning,
            info,
            git_added,
            git_modified,
            git_deleted,
            git_untracked,
            git_conflict,
            diff_add_bg,
            diff_del_bg,
            search_current_bg,
            search_current_fg,
            search_other_bg,
        );
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

/// Canonical id for a configured preset name, or `""` when nothing matches
/// (callers fall back to the default rather than failing to start).
fn normalize_preset_name(name: &str) -> &'static str {
    let normalized = name.trim().to_ascii_lowercase().replace('-', "_");
    match normalized.as_str() {
        // Names from before the presets had ids of their own.
        "dark" => return "ansi_dark",
        "light" => return "ansi_light",
        _ => {}
    }
    PRESETS
        .iter()
        .find(|preset| preset.id == normalized)
        .map(|preset| preset.id)
        .unwrap_or("")
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

/// The nine slots the capture table actually paints with. A preset is this
/// plus a [`UiColors`] — nothing else, so adding a theme is filling in two
/// structs rather than restating ~60 captures.
#[derive(Clone, Copy, Debug)]
pub struct SyntaxPalette {
    pub magenta: Color,
    pub blue: Color,
    pub cyan: Color,
    pub green: Color,
    pub yellow: Color,
    pub red: Color,
    /// Plain identifiers, operators, punctuation.
    pub text: Color,
    /// Comments.
    pub comment: Color,
    /// Between `text` and `comment`: de-emphasised but not a comment.
    pub muted: Color,
}

impl SyntaxPalette {
    /// Resolve one ANSI name from the base capture table. Names the table
    /// never uses pass through untouched, so a user override written as an
    /// ANSI name survives the remap.
    fn resolve(&self, color: Color) -> Color {
        match color {
            Color::Magenta => self.magenta,
            Color::Blue => self.blue,
            Color::Cyan => self.cyan,
            Color::Green => self.green,
            Color::Yellow => self.yellow,
            Color::Red => self.red,
            Color::White => self.text,
            Color::DarkGrey => self.comment,
            Color::Grey => self.muted,
            other => other,
        }
    }
}

const fn hex(r: u8, g: u8, b: u8) -> Color {
    Color::Rgb { r, g, b }
}

pub const PALETTE_DARK: SyntaxPalette = SyntaxPalette {
    magenta: hex(0xbb, 0x9a, 0xf7),
    blue: hex(0x7a, 0xa2, 0xf7),
    cyan: hex(0x7d, 0xcf, 0xff),
    green: hex(0x9e, 0xce, 0x6a),
    yellow: hex(0xe0, 0xaf, 0x68),
    red: hex(0xf7, 0x76, 0x8e),
    text: hex(0xc0, 0xc8, 0xe8),
    comment: hex(0x56, 0x5f, 0x89),
    muted: hex(0x8d, 0x96, 0xb8),
};

pub const PALETTE_LIGHT: SyntaxPalette = SyntaxPalette {
    magenta: hex(0x7a, 0x3d, 0xb8),
    blue: hex(0x2e, 0x5c, 0xc4),
    cyan: hex(0x16, 0x6b, 0xa8),
    green: hex(0x38, 0x7a, 0x2f),
    yellow: hex(0x9a, 0x62, 0x00),
    red: hex(0xc0, 0x2b, 0x45),
    text: hex(0x34, 0x38, 0x4a),
    // Readable on near-white but clearly secondary.
    comment: hex(0x8a, 0x90, 0xa4),
    muted: hex(0x5f, 0x66, 0x7d),
};

pub const PALETTE_DIM: SyntaxPalette = SyntaxPalette {
    magenta: hex(0xa5, 0x8c, 0xc9),
    blue: hex(0x7b, 0x9b, 0xd1),
    cyan: hex(0x6f, 0xb4, 0xc9),
    green: hex(0x8f, 0xb8, 0x7a),
    yellow: hex(0xcf, 0xa4, 0x6a),
    red: hex(0xd1, 0x79, 0x8a),
    text: hex(0xb0, 0xb6, 0xc4),
    comment: hex(0x5f, 0x66, 0x75),
    muted: hex(0x85, 0x8c, 0x9c),
};

pub const PALETTE_CONTRAST: SyntaxPalette = SyntaxPalette {
    magenta: hex(0xe0, 0xa6, 0xff),
    blue: hex(0x8a, 0xb6, 0xff),
    cyan: hex(0x6f, 0xdc, 0xff),
    green: hex(0x7c, 0xf0, 0x8a),
    yellow: hex(0xff, 0xd1, 0x66),
    red: hex(0xff, 0x7f, 0x92),
    text: hex(0xff, 0xff, 0xff),
    comment: hex(0x9a, 0xa0, 0xa6),
    muted: hex(0xc8, 0xc8, 0xc8),
};

pub const PALETTE_SEPIA: SyntaxPalette = SyntaxPalette {
    magenta: hex(0x7a, 0x46, 0x99),
    blue: hex(0x2f, 0x5f, 0x9e),
    cyan: hex(0x1f, 0x6f, 0x8b),
    green: hex(0x4a, 0x7a, 0x2f),
    yellow: hex(0x97, 0x60, 0x0b),
    red: hex(0xa8, 0x32, 0x2f),
    text: hex(0x3b, 0x33, 0x2a),
    comment: hex(0x8c, 0x81, 0x72),
    muted: hex(0x6b, 0x61, 0x53),
};

/// A selectable theme. `build` is what `[theme] preset = "<id>"` runs; `label`
/// is what the palette shows; `is_dark` is what OS appearance following needs.
pub struct ThemePreset {
    pub id: &'static str,
    pub label: &'static str,
    pub is_dark: bool,
    build: fn() -> Theme,
}

/// Every built-in preset, in the order the palette offers them. Single source
/// of truth: config resolution, the palette list and the light/dark split all
/// read this, so a preset cannot exist in one of them and not the others.
pub static PRESETS: &[ThemePreset] = &[
    ThemePreset {
        id: "ansi_dark",
        label: "ANSI Dark (terminal palette)",
        is_dark: true,
        build: Theme::ansi_dark,
    },
    ThemePreset {
        id: "ansi_light",
        label: "ANSI Light (terminal palette)",
        is_dark: false,
        build: Theme::ansi_light,
    },
    ThemePreset {
        id: "gargo_dark",
        label: "Gargo Dark",
        is_dark: true,
        build: Theme::gargo_dark,
    },
    ThemePreset {
        id: "gargo_light",
        label: "Gargo Light",
        is_dark: false,
        build: Theme::gargo_light,
    },
    ThemePreset {
        id: "gargo_dim",
        label: "Gargo Dim (low contrast)",
        is_dark: true,
        build: Theme::gargo_dim,
    },
    ThemePreset {
        id: "gargo_contrast",
        label: "Gargo High Contrast",
        is_dark: true,
        build: Theme::gargo_contrast,
    },
    ThemePreset {
        id: "gargo_sepia",
        label: "Gargo Sepia (warm light)",
        is_dark: false,
        build: Theme::gargo_sepia,
    },
];

/// Look up a preset by id, accepting the aliases `normalize_preset_name` knows.
pub fn find_preset(name: &str) -> Option<&'static ThemePreset> {
    let id = normalize_preset_name(name);
    PRESETS.iter().find(|preset| preset.id == id)
}

impl ThemePreset {
    pub fn build(&self) -> Theme {
        (self.build)()
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
    fn hex_presets_keep_every_capture_of_the_ansi_preset() {
        let ansi = Theme::ansi_dark();
        for (name, style) in &ansi.mappings {
            for hex in [Theme::gargo_dark(), Theme::gargo_light()] {
                let mapped = hex
                    .style_for_capture(name)
                    .unwrap_or_else(|| panic!("{name} missing from hex preset"));
                assert_eq!(mapped.bold, style.bold, "{name} bold");
                assert_eq!(mapped.italic, style.italic, "{name} italic");
                // Captures carrying only bold/italic (`emphasis`, `strong`)
                // have no color to remap and must stay colorless.
                assert_eq!(
                    style.fg.is_some(),
                    mapped.fg.is_some(),
                    "{name} gained or lost a color"
                );
                assert!(
                    style.fg.is_none() || matches!(mapped.fg, Some(Color::Rgb { .. })),
                    "{name} should resolve to rgb, got {:?}",
                    mapped.fg
                );
            }
        }
    }

    #[test]
    fn ui_colors_are_rgb_even_for_the_ansi_presets() {
        // The terminal palette decides how code looks; it must not decide
        // whether the status bar is readable.
        for theme in [Theme::ansi_dark(), Theme::ansi_light()] {
            assert!(matches!(theme.ui.status_bg, Color::Rgb { .. }));
            assert!(matches!(theme.ui.accent, Color::Rgb { .. }));
            assert!(matches!(theme.ui.border(), Color::Rgb { .. }));
        }
    }

    #[test]
    fn ansi_light_preset_uses_the_light_ui_palette() {
        assert_eq!(Theme::ansi_light().ui, UiColors::light());
        assert_eq!(Theme::ansi_dark().ui, UiColors::dark());
        assert_eq!(Theme::gargo_light().ui, UiColors::light());
    }

    #[test]
    fn every_preset_builds_and_is_reachable_by_id() {
        for preset in PRESETS {
            let theme = preset.build();
            assert!(
                theme.style_for_capture("keyword").is_some(),
                "{} has no captures",
                preset.id
            );
            assert_eq!(
                find_preset(preset.id).map(|p| p.id),
                Some(preset.id),
                "{} is not reachable by its own id",
                preset.id
            );
            // Hyphens and case are accepted for every id, not just some.
            assert_eq!(
                find_preset(&preset.id.replace('_', "-").to_uppercase()).map(|p| p.id),
                Some(preset.id)
            );
        }
    }

    #[test]
    fn preset_ids_are_unique() {
        let mut ids: Vec<&str> = PRESETS.iter().map(|p| p.id).collect();
        let before = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), before, "duplicate preset id");
    }

    #[test]
    fn presets_cover_both_appearances() {
        // OS appearance following needs at least one of each.
        assert!(PRESETS.iter().any(|p| p.is_dark));
        assert!(PRESETS.iter().any(|p| !p.is_dark));
    }

    #[test]
    fn hex_presets_resolve_every_ansi_slot() {
        // A palette that forgot a slot would leave that capture ANSI-colored
        // and quietly terminal-dependent.
        for preset in PRESETS.iter().filter(|p| !p.id.starts_with("ansi_")) {
            let theme = preset.build();
            for (name, style) in &theme.mappings {
                if let Some(fg) = style.fg {
                    assert!(
                        matches!(fg, Color::Rgb { .. }),
                        "{} left {name} as {fg:?}",
                        preset.id
                    );
                }
            }
        }
    }

    #[test]
    fn legacy_preset_names_still_resolve() {
        assert_eq!(find_preset("dark").map(|p| p.id), Some("ansi_dark"));
        assert_eq!(find_preset("light").map(|p| p.id), Some("ansi_light"));
    }

    #[test]
    fn unknown_preset_falls_back_to_the_default() {
        let cfg: Config = toml::from_str(
            r#"
[theme]
preset = "no_such_preset"
"#,
        )
        .unwrap();
        let theme = Theme::from_config(&cfg.theme);
        assert_eq!(
            theme.style_for_capture("keyword").and_then(|s| s.fg),
            Some(Color::Magenta)
        );
    }

    #[test]
    fn from_config_applies_ui_role_overrides() {
        let cfg: Config = toml::from_str(
            r##"
[theme]
preset = "gargo_dark"

[theme.ui]
accent = "#ff0000"
git_added = "#00ff00"
bad = "#000000"
"##,
        )
        .unwrap();

        let theme = Theme::from_config(&cfg.theme);
        assert_eq!(
            theme.ui.accent,
            Color::Rgb {
                r: 0xff,
                g: 0,
                b: 0
            }
        );
        // Derived roles follow the override instead of needing their own key.
        assert_eq!(
            theme.ui.gutter_added(),
            UiColors {
                git_added: Color::Rgb {
                    r: 0,
                    g: 0xff,
                    b: 0
                },
                ..UiColors::dark()
            }
            .gutter_added()
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
