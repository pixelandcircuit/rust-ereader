// use crate::CONTENT_ID;
use crate::font::{char_advance, draw_str, font_px_for, line_height, measure_width};
use crate::hardware::FontSize;
use crate::layout::{FontMetrics, LayoutConfig};
use crate::reader::BookSession;
#[cfg(feature = "esp")]
use alloc::boxed::Box;
#[cfg(feature = "esp")]
use alloc::format;
#[cfg(feature = "esp")]
use alloc::string::String;
use embedded_graphics_core::pixelcolor::{Rgb565, RgbColor};
use fontdue::Font;
use iris_ui::geom::Bounds;
use iris_ui::scene::Scene;
use iris_ui::view::ViewId;
use iris_ui::DrawEvent;

pub const CONTENT_ID: ViewId = ViewId::new("content");

pub struct BookState {
    pub text: String,
    pub font_px: f32,
    pub font: &'static fontdue::Font,
    pub heading_font: &'static fontdue::Font,
    pub heading_font_px: f32,
}

pub fn draw_book_content(e: &mut DrawEvent) {
    let bounds = e.view.bounds;
    // get_state returns &mut BookState which borrows e.view. We need e.ctx in the
    // pixel closure below, but can't hold both borrows simultaneously through e.
    // Extract a raw pointer to renderer (safe: BookState lives in a Box inside the
    // view for the entire function; we never replace the state box during rendering).
    if let Some(state) = e.view.get_state::<BookState>() {
        e.ctx.fill_rect(&bounds, &Rgb565::WHITE);
        render_ttf_text(
            state.font,
            state.heading_font,
            &state.text,
            state.font_px,
            state.heading_font_px,
            bounds,
            |px, py, g4| {
                let gray8 = (g4 << 4) | g4;
                let v5 = (gray8 >> 3) as u8;
                let v6 = (gray8 >> 2) as u8;
                e.ctx.put_pixel(px, py, &Rgb565::new(v5, v6, v5));
            },
        );
        e.ctx.stroke_rect(&bounds, &Rgb565::BLACK);
    }
}

pub fn update_content(scene: &mut Scene, session: &BookSession, font_px: f32) {
    let chapter_str = format!(
        "Ch.{} of {}",
        session.chapter_idx + 1,
        session.chapter_count()
    );
    if let Some(v) = scene.get_view_mut(&ViewId::new("chapter")) {
        v.title = chapter_str;
    }
    let page_str = format!(
        "p.{}/{}",
        session.reader.current_page + 1,
        session.reader.page_count()
    );
    if let Some(v) = scene.get_view_mut(&ViewId::new("page")) {
        v.title = page_str;
    }
    if let Some(v) = scene.get_view_mut(&CONTENT_ID) {
        if let Some(state) = v.get_state::<BookState>() {
            state.text = session.reader.current_text().into();
            state.font_px = font_px;
        }
    }
    scene.mark_dirty_all();
}

/// Build a LayoutConfig for Noticia Text at the given font size using real TTF
/// metrics so that layout and rendering agree on where line breaks fall and how
/// many lines fit per page.
///
/// `content_w` / `content_h` are the **actual pixel dimensions of the content
/// view** as reported by the UI layout engine.  Pass `scene.get_view_bounds(&CONTENT_ID)`
/// rather than the raw screen size — the function subtracts the renderer's
/// internal padding (16 px each side horizontally, 12 px each side vertically)
/// so layout and render agree on how many characters/lines fit.
pub fn layout_cfg(
    font: &'static Font,
    heading_font: &'static Font,
    font_size: FontSize,
    content_w: i32,
    content_h: i32,
) -> LayoutConfig {
    let font_px = font_px_for(font_size);
    let line_h = (line_height(font, font_px) as u32).saturating_add(4);
    let space_w = char_advance(font, ' ', font_px) as u32;

    let heading_px = font_px * 1.4;
    let heading_line_h = (line_height(heading_font, heading_px) as u32).saturating_add(4);
    let heading_space_w = char_advance(heading_font, ' ', heading_px) as u32;

    // render_ttf_text uses pad_x=16 on each side and pad_y=12 on each side.
    let text_w = (content_w as u32).saturating_sub(32);
    let text_h = (content_h as u32).saturating_sub(24);

    LayoutConfig {
        screen_width: text_w,
        screen_height: text_h,
        margin_x: 0,
        margin_y: 0,
        font: FontMetrics {
            line_height_px: line_h,
            space_width_px: space_w,
            measure: Box::new(move |s: &str| measure_width(font, s, font_px).max(0) as u32),
        },
        heading_font: Some(FontMetrics {
            line_height_px: heading_line_h,
            space_width_px: heading_space_w,
            measure: Box::new(move |s: &str| {
                measure_width(heading_font, s, heading_px).max(0) as u32
            }),
        }),
    }
}

/// Word-wrap one line of text to fit `max_px` pixels wide at the given TTF size.
/// Handles hard newlines; advances past trailing spaces on the remainder.
fn next_ttf_line<'a>(
    font: &'static Font,
    text: &'a str,
    max_px: i32,
    font_px: f32,
) -> (&'a str, &'a str) {
    let mut cursor = 0.0f32;
    let mut last_space: Option<usize> = None;
    for (i, c) in text.char_indices() {
        // Hard newline: break here regardless of width.
        if c == '\n' {
            let after = text[i + 1..].trim_start_matches('\r');
            return (text[..i].trim_end(), after);
        }
        let adv = char_advance(font, c, font_px);
        if cursor + adv > max_px as f32 + 0.5 {
            return if let Some(sp) = last_space {
                (text[..sp].trim_end(), text[sp..].trim_start())
            } else {
                (&text[..i], &text[i..]) // force break mid-word
            };
        }
        if c == ' ' {
            last_space = Some(i);
        }
        cursor += adv;
    }
    (text.trim_end(), "")
}

/// Render page text with Noticia Text TTF, emitting one (x, y, gray4) pixel
/// at a time to `put_pixel`. Handles word-wrap, padding, and bounds clipping.
fn render_ttf_text(
    font: &'static Font,
    heading_font: &'static Font,
    text: &str,
    font_px: f32,
    heading_font_px: f32,
    bounds: Bounds,
    mut put_pixel: impl FnMut(i32, i32, u8),
) {
    if text.is_empty() {
        return;
    }
    let pad_x = 16i32;
    let pad_y = 12i32;
    let cx = bounds.position.x;
    let cy = bounds.position.y;
    let cw = bounds.size.w;
    let ch = bounds.size.h;
    let max_px = cw - pad_x * 2;

    // Detect whether the current paragraph is a heading by checking for a
    // sentinel byte (\x01–\x03) at the start of the paragraph.
    fn is_sentinel(s: &str) -> bool {
        s.as_bytes().first().map_or(false, |&b| b >= 1 && b <= 3)
    }
    fn strip_sentinel(s: &str) -> &str {
        if s.as_bytes().first().map_or(false, |&b| b >= 1 && b <= 3) {
            &s[1..]
        } else {
            s
        }
    }

    let mut in_heading = is_sentinel(text);
    let mut current_font = if in_heading { heading_font } else { font };
    let mut current_px = if in_heading { heading_font_px } else { font_px };
    let mut line_h = line_height(current_font, current_px) + 4;
    let mut baseline = cy + pad_y + line_height(current_font, current_px);
    let mut remaining = strip_sentinel(text);

    while !remaining.is_empty() && baseline < cy + ch - pad_y {
        let (line, rest) = next_ttf_line(current_font, remaining, max_px, current_px);
        if !line.is_empty() {
            draw_str(
                current_font,
                line,
                cx + pad_x,
                baseline,
                current_px,
                15,
                &mut |px, py, g4| {
                    if px >= cx && px < cx + cw && py >= cy && py < cy + ch {
                        put_pixel(px, py, g4);
                    }
                },
            );
        }
        remaining = rest;
        if remaining.starts_with('\n') {
            remaining = &remaining[1..];
            baseline += line_h * crate::layout::PARA_GAP_LINES as i32;
            // Detect next paragraph's heading status and update font/size.
            in_heading = is_sentinel(remaining);
            remaining = strip_sentinel(remaining);
            current_font = if in_heading { heading_font } else { font };
            current_px = if in_heading { heading_font_px } else { font_px };
            line_h = line_height(current_font, current_px) + 4;
        } else {
            baseline += line_h;
        }
    }
}
