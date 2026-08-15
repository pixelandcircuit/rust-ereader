use crate::hardware::FontSize;
#[cfg(feature = "esp")]
use alloc::boxed::Box;

/// All font faces used by the app, bundled so they can be threaded through
/// the call stack without growing every function signature individually.
pub struct AppFonts {
    pub ui: &'static fontdue::Font,
    pub ui_bold: &'static fontdue::Font,
    pub body: &'static fontdue::Font,
    pub body_bold: &'static fontdue::Font,
    pub body_italic: &'static fontdue::Font,
}

impl Copy for AppFonts {}
impl Clone for AppFonts {
    fn clone(&self) -> Self {
        *self
    }
}

/// Draw `text` with its baseline at (start_x, baseline_y).
/// `bg` is the background Gray4 value (15 = white). Returns the x position after the last glyph.
/// `draw` receives (x: i32, y: i32, gray4: u8) for each non-transparent pixel.
pub fn draw_str(
    font: &fontdue::Font,
    text: &str,
    start_x: i32,
    baseline_y: i32,
    font_px: f32,
    bg: u8,
    draw: &mut dyn FnMut(i32, i32, u8),
) -> i32 {
    let mut cursor_x = start_x as f32;
    for c in text.chars() {
        let (metrics, bitmap) = font.rasterize(c, font_px);
        let gx = cursor_x + metrics.xmin as f32;
        let gy = baseline_y - metrics.ymin - metrics.height as i32;
        for (idx, &cov) in bitmap.iter().enumerate() {
            if cov == 0 {
                continue;
            }
            let px = gx as i32 + (idx % metrics.width) as i32;
            let py = gy + (idx / metrics.width) as i32;
            draw(px, py, blend_gray4(cov, 0, bg));
        }
        cursor_x += metrics.advance_width;
    }
    cursor_x as i32
}

/// Measure the pixel width of `text` at the given font size without rasterizing bitmaps.
pub fn measure_width(font: &fontdue::Font, text: &str, font_px: f32) -> i32 {
    let mut width = 0.0f32;
    for c in text.chars() {
        width += font.metrics(c, font_px).advance_width;
    }
    width as i32
}

/// Advance width of a single character at the given font size (no bitmap allocated).
pub fn char_advance(font: &fontdue::Font, c: char, font_px: f32) -> f32 {
    font.metrics(c, font_px).advance_width
}

/// Recommended line height (ascent - descent) at the given font size.
pub fn line_height(font: &fontdue::Font, font_px: f32) -> i32 {
    match font.horizontal_line_metrics(font_px) {
        Some(m) => (m.ascent - m.descent) as i32,
        None => font_px as i32,
    }
}

/// Blend coverage (0=transparent, 255=opaque) against fg/bg Gray4 values.
#[inline(always)]
fn blend_gray4(coverage: u8, fg_g4: u8, bg_g4: u8) -> u8 {
    let a = coverage as u16;
    ((fg_g4 as u16 * a + bg_g4 as u16 * (255 - a)) / 255) as u8
}

/// TTF font size in pixels for each FontSize option.
pub fn font_px_for(size: FontSize) -> f32 {
    match size {
        FontSize::Small => 22.0,
        FontSize::Medium => 28.0,
        FontSize::Large => 30.0,
    }
}
