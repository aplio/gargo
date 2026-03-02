use std::io::{self, Write};
use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
};

use crossterm::{
    cursor::MoveTo,
    queue,
    style::{Attribute, Print, SetAttribute},
};
use unicode_width::UnicodeWidthStr;

use crate::core_lib::ui::style::{CellStyle, apply_style};
use crate::core_lib::ui::surface::Surface;
use crate::core_lib::ui::url::{UrlSpan, find_web_url_spans, write_hyperlink_close, write_hyperlink_open};

struct RowUrlMap {
    text: String,
    spans: Vec<UrlSpan>,
    byte_by_col: Vec<Option<usize>>,
    url_key_by_col: Vec<Option<UrlKey>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct UrlKey {
    start: usize,
    end: usize,
    hash: u64,
}

impl UrlKey {
    fn from_url_span(span: UrlSpan, text: &str) -> Self {
        let url = span.as_str(text);
        let mut hasher = DefaultHasher::new();
        url.hash(&mut hasher);
        Self {
            start: span.start,
            end: span.end,
            hash: hasher.finish(),
        }
    }
}

impl RowUrlMap {
    fn from_surface_row(surface: &Surface, y: usize) -> Self {
        let mut text = String::new();
        let mut byte_by_col = vec![None; surface.width];
        for (x, slot) in byte_by_col.iter_mut().enumerate() {
            let symbol = surface.get(x, y).symbol.as_str();
            if symbol.is_empty() {
                continue;
            }
            *slot = Some(text.len());
            text.push_str(symbol);
        }
        let spans = find_web_url_spans(&text);
        let span_keys = spans
            .iter()
            .map(|span| UrlKey::from_url_span(*span, &text))
            .collect::<Vec<_>>();
        let mut url_key_by_col = vec![None; surface.width];
        for (col, slot) in url_key_by_col.iter_mut().enumerate() {
            let Some(byte) = byte_by_col[col] else {
                continue;
            };
            if let Some((idx, _)) = spans
                .iter()
                .enumerate()
                .find(|(_, span)| span.contains_byte(byte))
            {
                *slot = Some(span_keys[idx]);
            }
        }
        Self {
            text,
            spans,
            byte_by_col,
            url_key_by_col,
        }
    }

    fn url_for_col(&self, col: usize) -> Option<&str> {
        let byte = self.byte_by_col.get(col).copied().flatten()?;
        self.spans
            .iter()
            .find(|span| span.contains_byte(byte))
            .map(|span| span.as_str(&self.text))
    }

    fn url_key_for_col(&self, col: usize) -> Option<UrlKey> {
        self.url_key_by_col.get(col).copied().flatten()
    }
}

/// Draw only the cells that differ between previous and current surface.
pub fn draw_diff(prev: &Surface, curr: &Surface, stdout: &mut impl Write) -> io::Result<()> {
    let width = curr.width;
    let height = curr.height;

    let mut last_style: Option<&CellStyle> = None;
    let mut last_col: Option<usize> = None;
    let mut last_row: Option<usize> = None;

    for y in 0..height {
        let row_urls = RowUrlMap::from_surface_row(curr, y);
        let prev_row_urls = RowUrlMap::from_surface_row(prev, y);
        let mut active_url: Option<String> = None;

        for x in 0..width {
            let curr_cell = curr.get(x, y);
            let prev_cell = prev.get(x, y);
            let url_changed = row_urls.url_key_for_col(x) != prev_row_urls.url_key_for_col(x);

            if curr_cell == prev_cell && !url_changed {
                if active_url.take().is_some() {
                    write_hyperlink_close(stdout)?;
                }
                last_col = None;
                continue;
            }

            if curr_cell.symbol.is_empty() {
                if active_url.take().is_some() {
                    write_hyperlink_close(stdout)?;
                }
                last_col = None;
                continue;
            }

            let need_move = match (last_col, last_row) {
                (Some(lc), Some(lr)) => lc != x || lr != y,
                _ => true,
            };
            if need_move {
                if active_url.take().is_some() {
                    write_hyperlink_close(stdout)?;
                }
                queue!(stdout, MoveTo(x as u16, y as u16))?;
            }

            let style = &curr_cell.style;
            let style_changed = last_style != Some(style);
            if style_changed {
                apply_style(stdout, *style)?;
                last_style = Some(style);
            }

            let url_for_cell = row_urls.url_for_col(x);
            if active_url.as_deref() != url_for_cell {
                if active_url.take().is_some() {
                    write_hyperlink_close(stdout)?;
                }
                if let Some(url) = url_for_cell {
                    write_hyperlink_open(stdout, url)?;
                    active_url = Some(url.to_string());
                }
            }

            queue!(stdout, Print(&curr_cell.symbol))?;

            let char_width = if curr_cell.symbol.is_empty() {
                1
            } else {
                UnicodeWidthStr::width(curr_cell.symbol.as_str())
            };
            last_col = Some(x + char_width);
            last_row = Some(y);
        }

        if active_url.is_some() {
            write_hyperlink_close(stdout)?;
        }
    }

    if last_style.is_some() {
        queue!(stdout, SetAttribute(Attribute::Reset))?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core_lib::ui::surface::Surface;

    #[test]
    fn draw_diff_wraps_detected_web_urls_in_osc8_hyperlinks() {
        let prev = Surface::new(48, 1);
        let mut curr = Surface::new(48, 1);
        curr.put_str(
            0,
            0,
            "visit https://example.com/docs now",
            &CellStyle::default(),
        );

        let mut out = Vec::new();
        draw_diff(&prev, &curr, &mut out).expect("draw diff");
        let rendered = String::from_utf8(out).expect("utf8");
        assert!(
            rendered.contains(
                "\u{1b}]8;;https://example.com/docs\u{1b}\\https://example.com/docs\u{1b}]8;;\u{1b}\\"
            ),
            "expected hyperlink sequence in output, got: {rendered:?}"
        );
    }

    #[test]
    fn draw_diff_rewrites_unchanged_url_prefix_when_target_changes() {
        let mut prev = Surface::new(32, 1);
        prev.put_str(0, 0, "https://example.com/a", &CellStyle::default());
        let mut curr = Surface::new(32, 1);
        curr.put_str(0, 0, "https://example.com/b", &CellStyle::default());

        let mut out = Vec::new();
        draw_diff(&prev, &curr, &mut out).expect("draw diff");
        let rendered = String::from_utf8(out).expect("utf8");
        assert!(
            rendered.contains(
                "\u{1b}]8;;https://example.com/b\u{1b}\\https://example.com/b\u{1b}]8;;\u{1b}\\"
            ),
            "expected full rewritten hyperlink target in output, got: {rendered:?}"
        );
    }
}
