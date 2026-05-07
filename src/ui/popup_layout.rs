use std::sync::atomic::{AtomicU8, Ordering};

const DEFAULT_POPUP_WIDTH_PERCENT: u8 = 95;
const DEFAULT_POPUP_HEIGHT_PERCENT: u8 = 90;
const MIN_POPUP_DIMENSION: usize = 3;

static POPUP_WIDTH_PERCENT: AtomicU8 = AtomicU8::new(DEFAULT_POPUP_WIDTH_PERCENT);
static POPUP_HEIGHT_PERCENT: AtomicU8 = AtomicU8::new(DEFAULT_POPUP_HEIGHT_PERCENT);

/// Configure the popup-size percentages from `Config::ui`. Values outside
/// [10, 100] are clamped; pass 80 to mirror the historical default.
pub fn set_popup_percent(width: u8, height: u8) {
    POPUP_WIDTH_PERCENT.store(width.clamp(10, 100), Ordering::Relaxed);
    POPUP_HEIGHT_PERCENT.store(height.clamp(10, 100), Ordering::Relaxed);
}

pub fn popup_width_percent() -> u8 {
    POPUP_WIDTH_PERCENT.load(Ordering::Relaxed)
}

pub fn popup_height_percent() -> u8 {
    POPUP_HEIGHT_PERCENT.load(Ordering::Relaxed)
}

/// Compute (popup_w, popup_h) for an overlay given the terminal dimensions.
/// Honors the configured percentages and the historical floor of 3 cells.
pub fn popup_size(cols: usize, rows: usize) -> (usize, usize) {
    let w_pct = popup_width_percent() as usize;
    let h_pct = popup_height_percent() as usize;
    let w = (cols * w_pct / 100).max(MIN_POPUP_DIMENSION);
    let h = (rows * h_pct / 100).max(MIN_POPUP_DIMENSION);
    (w, h)
}
