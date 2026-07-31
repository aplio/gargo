//! UI chrome colors — everything the editor paints that is not a syntax token.
//!
//! Kept as true RGB rather than ANSI names on purpose: an ANSI name resolves to
//! whatever the user's terminal palette says, so the same theme looked like a
//! different editor in every terminal, and eight hues are not enough to tell
//! apart "changed", "selected" and "focused" at a glance.

use crossterm::style::Color;

/// Colors for UI chrome. Roles are the *meaning* of a color, not its position:
/// `accent` is "this is the thing you are looking at", not "cyan".
///
/// Roles that are a **relationship** between two other roles are not fields —
/// they are computed (see [`UiColors::border`]). A rule line between a panel
/// and its text is not a color anyone chooses; making every preset spell it out
/// just means every preset gets it slightly wrong.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UiColors {
    /// Editor background. Also the base every derived role is mixed toward.
    pub bg: Color,
    /// Background of popups, sidebars and other panels raised above `bg`.
    pub panel_bg: Color,
    /// Default text.
    pub text: Color,
    /// Secondary text: metadata, timestamps, inactive labels.
    pub dim: Color,
    /// Tertiary text: line numbers, separators, hints.
    pub faint: Color,
    /// The thing you are looking at: headers, selected markers, links.
    pub accent: Color,
    /// Background of the selected row in a list.
    pub selected_bg: Color,
    /// Text on `selected_bg`.
    pub selected_fg: Color,
    /// Directory names in trees and pickers.
    pub folder: Color,
    /// Unsaved-changes marker.
    pub dirty: Color,

    /// Status bar.
    pub status_bg: Color,
    pub status_fg: Color,
    /// Text drawn on any of the `mode_*` backgrounds.
    pub mode_fg: Color,
    pub mode_normal: Color,
    pub mode_insert: Color,
    pub mode_visual: Color,

    pub error: Color,
    pub warning: Color,
    pub info: Color,

    pub git_added: Color,
    pub git_modified: Color,
    pub git_deleted: Color,
    pub git_untracked: Color,
    pub git_conflict: Color,

    /// Backgrounds tinting a side-by-side diff.
    pub diff_add_bg: Color,
    pub diff_del_bg: Color,

    /// The search match the cursor is on, and the rest of them.
    pub search_current_bg: Color,
    pub search_current_fg: Color,
    pub search_other_bg: Color,
}

impl Default for UiColors {
    fn default() -> Self {
        Self::dark()
    }
}

impl UiColors {
    pub fn dark() -> Self {
        Self {
            bg: rgb(0x1a, 0x1b, 0x26),
            panel_bg: rgb(0x1f, 0x22, 0x35),
            text: rgb(0xc0, 0xc8, 0xe8),
            dim: rgb(0x8d, 0x96, 0xb8),
            faint: rgb(0x56, 0x5f, 0x89),
            accent: rgb(0x7d, 0xcf, 0xff),
            selected_bg: rgb(0x2e, 0x34, 0x4f),
            selected_fg: rgb(0xdc, 0xe2, 0xf7),
            folder: rgb(0x7a, 0xa2, 0xf7),
            dirty: rgb(0xe0, 0xaf, 0x68),

            status_bg: rgb(0x2a, 0x2f, 0x41),
            status_fg: rgb(0xc0, 0xc8, 0xe8),
            mode_fg: rgb(0x1a, 0x1b, 0x26),
            mode_normal: rgb(0x7a, 0xa2, 0xf7),
            mode_insert: rgb(0x9e, 0xce, 0x6a),
            mode_visual: rgb(0xbb, 0x9a, 0xf7),

            error: rgb(0xf7, 0x76, 0x8e),
            warning: rgb(0xe0, 0xaf, 0x68),
            info: rgb(0x7d, 0xcf, 0xff),

            git_added: rgb(0x9e, 0xce, 0x6a),
            git_modified: rgb(0xe0, 0xaf, 0x68),
            git_deleted: rgb(0xf7, 0x76, 0x8e),
            git_untracked: rgb(0x73, 0xda, 0xca),
            git_conflict: rgb(0xbb, 0x9a, 0xf7),

            diff_add_bg: rgb(0x20, 0x36, 0x2a),
            diff_del_bg: rgb(0x3b, 0x22, 0x2c),

            search_current_bg: rgb(0xe0, 0xaf, 0x68),
            search_current_fg: rgb(0x1a, 0x1b, 0x26),
            search_other_bg: rgb(0x54, 0x45, 0x2c),
        }
    }

    pub fn light() -> Self {
        Self {
            bg: rgb(0xfa, 0xfa, 0xfb),
            panel_bg: rgb(0xf0, 0xf1, 0xf6),
            text: rgb(0x34, 0x38, 0x4a),
            dim: rgb(0x5f, 0x66, 0x7d),
            faint: rgb(0x99, 0xa0, 0xb4),
            accent: rgb(0x16, 0x6b, 0xa8),
            selected_bg: rgb(0xdd, 0xe3, 0xf2),
            selected_fg: rgb(0x1c, 0x20, 0x2e),
            folder: rgb(0x2e, 0x5c, 0xc4),
            dirty: rgb(0x9a, 0x62, 0x00),

            status_bg: rgb(0xe4, 0xe7, 0xef),
            status_fg: rgb(0x34, 0x38, 0x4a),
            mode_fg: rgb(0xfa, 0xfa, 0xfb),
            mode_normal: rgb(0x2e, 0x5c, 0xc4),
            mode_insert: rgb(0x38, 0x7a, 0x2f),
            mode_visual: rgb(0x7a, 0x3d, 0xb8),

            error: rgb(0xc0, 0x2b, 0x45),
            warning: rgb(0x9a, 0x62, 0x00),
            info: rgb(0x16, 0x6b, 0xa8),

            git_added: rgb(0x38, 0x7a, 0x2f),
            git_modified: rgb(0x9a, 0x62, 0x00),
            git_deleted: rgb(0xc0, 0x2b, 0x45),
            git_untracked: rgb(0x0f, 0x76, 0x6e),
            git_conflict: rgb(0x7a, 0x3d, 0xb8),

            diff_add_bg: rgb(0xdd, 0xf0, 0xdb),
            diff_del_bg: rgb(0xfa, 0xdf, 0xe4),

            search_current_bg: rgb(0xf5, 0xd0, 0x7a),
            search_current_fg: rgb(0x34, 0x38, 0x4a),
            search_other_bg: rgb(0xf0, 0xe6, 0xc8),
        }
    }

    /// Rule lines and panel frames. Derived from the distance between the
    /// surface and the text on it, so a theme cannot end up with a border that
    /// disappears into its own background.
    pub fn border(&self) -> Color {
        blend(self.bg, self.text, 0.30)
    }

    /// Sidebar surface: a step away from `panel_bg` so a sidebar next to a
    /// popup still reads as a separate plane.
    pub fn sidebar_bg(&self) -> Color {
        blend(self.panel_bg, self.text, 0.05)
    }

    /// The current line's background — a hint, not a highlight.
    pub fn current_line_bg(&self) -> Color {
        blend(self.bg, self.text, 0.08)
    }

    /// Change bars in the gutter. Muted against the editor background so a
    /// file with many changes doesn't turn into a christmas tree, while
    /// keeping the hue of the status it stands for.
    pub fn gutter_added(&self) -> Color {
        blend(self.bg, self.git_added, 0.55)
    }

    pub fn gutter_modified(&self) -> Color {
        blend(self.bg, self.git_modified, 0.55)
    }

    pub fn gutter_deleted(&self) -> Color {
        blend(self.bg, self.git_deleted, 0.55)
    }
}

const fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::Rgb { r, g, b }
}

fn channels(color: Color) -> Option<(u8, u8, u8)> {
    match color {
        Color::Rgb { r, g, b } => Some((r, g, b)),
        _ => None,
    }
}

/// Mix `t` of `to` into `from`. Falls back to `to` when either side is an ANSI
/// name (a user override can be one), since there is nothing to interpolate.
fn blend(from: Color, to: Color, t: f32) -> Color {
    let (Some((r1, g1, b1)), Some((r2, g2, b2))) = (channels(from), channels(to)) else {
        return to;
    };
    let mix = |a: u8, b: u8| {
        (a as f32 + (b as f32 - a as f32) * t)
            .round()
            .clamp(0.0, 255.0) as u8
    };
    Color::Rgb {
        r: mix(r1, r2),
        g: mix(g1, g2),
        b: mix(b1, b2),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derived_border_sits_between_background_and_text() {
        let ui = UiColors::dark();
        let (br, _, _) = channels(ui.border()).unwrap();
        let (bgr, _, _) = channels(ui.bg).unwrap();
        let (tr, _, _) = channels(ui.text).unwrap();
        assert!(
            br > bgr && br < tr,
            "border {br} not between {bgr} and {tr}"
        );
    }

    #[test]
    fn derived_roles_follow_a_changed_base() {
        // The point of deriving: override one role and everything built on it
        // moves with it, instead of drifting until someone notices.
        let mut ui = UiColors::dark();
        let before = ui.gutter_added();
        ui.git_added = rgb(0x00, 0xff, 0x00);
        assert_ne!(before, ui.gutter_added());
    }

    #[test]
    fn blend_falls_back_when_a_side_is_an_ansi_name() {
        assert_eq!(blend(Color::Cyan, rgb(1, 2, 3), 0.5), rgb(1, 2, 3));
        assert_eq!(blend(rgb(1, 2, 3), Color::Cyan, 0.5), Color::Cyan);
    }

    #[test]
    fn light_and_dark_differ_on_every_surface_role() {
        let (d, l) = (UiColors::dark(), UiColors::light());
        assert_ne!(d.bg, l.bg);
        assert_ne!(d.panel_bg, l.panel_bg);
        assert_ne!(d.text, l.text);
        assert_ne!(d.status_bg, l.status_bg);
    }
}
