// use crate::CONTENT_ID;
use crate::font::{char_advance, draw_str, font_px_for, line_height, measure_width, AppFonts};
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
use iris_ui::geom::Bounds;
use iris_ui::scene::Scene;
use iris_ui::view::ViewId;
use iris_ui::DrawEvent;

pub const CONTENT_ID: ViewId = ViewId::new("content");

pub struct BookState {
    pub text: String,
    pub font_px: f32,
    pub fonts: AppFonts,
    pub heading_font_px: f32,
}

pub fn draw_book_content(e: &mut DrawEvent<Rgb565>) {
    let bounds = e.view.bounds;
    if let Some(state) = e.view.get_state::<BookState>() {
        e.ctx.fill_rect(&bounds, &Rgb565::WHITE);
        render_ttf_text(
            state.fonts,
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

pub fn update_content(scene: &mut Scene<Rgb565>, session: &BookSession, font_px: f32) {
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

/// Build a LayoutConfig using real TTF metrics so that layout and rendering
/// agree on where line breaks fall and how many lines fit per page.
///
/// `content_w` / `content_h` are the **actual pixel dimensions of the content
/// view** as reported by the UI layout engine.  Pass `scene.get_view_bounds(&CONTENT_ID)`
/// rather than the raw screen size — the function subtracts the renderer's
/// internal padding (16 px each side horizontally, 12 px each side vertically)
/// so layout and render agree on how many characters/lines fit.
pub fn layout_cfg(
    fonts: &AppFonts,
    font_size: FontSize,
    content_w: i32,
    content_h: i32,
) -> LayoutConfig {
    let font_px = font_px_for(font_size);
    let line_h = (line_height(fonts.body, font_px) as u32).saturating_add(4);
    let space_w = char_advance(fonts.body, ' ', font_px) as u32;

    let heading_px = font_px * 1.4;
    let heading_line_h = (line_height(fonts.ui_bold, heading_px) as u32).saturating_add(4);
    let heading_space_w = char_advance(fonts.ui_bold, ' ', heading_px) as u32;

    let bold_space_w = char_advance(fonts.body_bold, ' ', font_px) as u32;
    let italic_space_w = char_advance(fonts.body_italic, ' ', font_px) as u32;

    // render_ttf_text uses pad_x=16 on each side and pad_y=12 on each side.
    let text_w = (content_w as u32).saturating_sub(32);
    let text_h = (content_h as u32).saturating_sub(24);

    // Capture individual font pointers (all &'static) for the measure closures.
    let body = fonts.body;
    let body_bold = fonts.body_bold;
    let body_italic = fonts.body_italic;
    let heading_font = fonts.ui_bold;

    LayoutConfig {
        screen_width: text_w,
        screen_height: text_h,
        margin_x: 0,
        margin_y: 0,
        font: FontMetrics {
            line_height_px: line_h,
            space_width_px: space_w,
            measure: Box::new(move |s: &str| measure_width(body, s, font_px).max(0) as u32),
        },
        bold_font: Some(FontMetrics {
            line_height_px: line_h,
            space_width_px: bold_space_w,
            measure: Box::new(move |s: &str| measure_width(body_bold, s, font_px).max(0) as u32),
        }),
        italic_font: Some(FontMetrics {
            line_height_px: line_h,
            space_width_px: italic_space_w,
            measure: Box::new(move |s: &str| measure_width(body_italic, s, font_px).max(0) as u32),
        }),
        heading_font: Some(FontMetrics {
            line_height_px: heading_line_h,
            space_width_px: heading_space_w,
            measure: Box::new(move |s: &str| {
                measure_width(heading_font, s, heading_px).max(0) as u32
            }),
        }),
    }
}

fn select_font(
    fonts: AppFonts,
    in_heading: bool,
    in_bold: bool,
    in_italic: bool,
) -> &'static fontdue::Font {
    if in_heading {
        fonts.ui_bold
    } else if in_bold {
        fonts.body_bold
    } else if in_italic {
        fonts.body_italic
    } else {
        fonts.body
    }
}

/// Word-wrap one line of text to fit `max_px` pixels wide.
///
/// Tracks inline bold/italic style via sentinel bytes (\x04–\x07) and uses the
/// correct font for each character's advance width. Returns `(line, rest,
/// new_in_bold, new_in_italic)` where the booleans reflect the style state at
/// the start of `rest` (end of `line`).
fn next_ttf_line(
    fonts: AppFonts,
    text: &str,
    max_px: i32,
    font_px: f32,
    heading_font_px: f32,
    in_heading: bool,
    mut in_bold: bool,
    mut in_italic: bool,
) -> (&str, &str, bool, bool) {
    let mut cursor = 0.0f32;
    let mut last_space: Option<usize> = None;
    // Style state saved at last word-break opportunity.
    let mut last_space_bold = in_bold;
    let mut last_space_italic = in_italic;

    for (i, c) in text.char_indices() {
        if c == '\n' {
            let after = text[i + 1..].trim_start_matches('\r');
            return (text[..i].trim_end(), after, in_bold, in_italic);
        }
        match c as u32 {
            0x04 => {
                in_bold = true;
                continue;
            }
            0x05 => {
                in_bold = false;
                continue;
            }
            0x06 => {
                in_italic = true;
                continue;
            }
            0x07 => {
                in_italic = false;
                continue;
            }
            _ => {}
        }
        let active_font = select_font(fonts, in_heading, in_bold, in_italic);
        let px = if in_heading { heading_font_px } else { font_px };
        let adv = char_advance(active_font, c, px);
        if cursor + adv > max_px as f32 + 0.5 {
            return if let Some(sp) = last_space {
                // Style sentinels between last_space and i are in `rest`; return
                // the state at last_space so the next line starts correctly.
                (
                    text[..sp].trim_end(),
                    text[sp..].trim_start(),
                    last_space_bold,
                    last_space_italic,
                )
            } else {
                (&text[..i], &text[i..], in_bold, in_italic)
            };
        }
        if c == ' ' {
            last_space = Some(i);
            last_space_bold = in_bold;
            last_space_italic = in_italic;
        }
        cursor += adv;
    }
    (text.trim_end(), "", in_bold, in_italic)
}

/// Render page text with TTF fonts, emitting one (x, y, gray4) pixel at a time
/// to `put_pixel`. Handles word-wrap, padding, bounds clipping, and inline
/// bold/italic switching via sentinel bytes.
fn render_ttf_text(
    fonts: AppFonts,
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
    let mut clipped = |px: i32, py: i32, g4: u8| {
        if px >= cx && px < cx + cw && py >= cy && py < cy + ch {
            put_pixel(px, py, g4);
        }
    };

    fn is_heading_sentinel(s: &str) -> bool {
        s.as_bytes().first().map_or(false, |&b| b >= 1 && b <= 3)
    }
    fn strip_heading_sentinel(s: &str) -> &str {
        if s.as_bytes().first().map_or(false, |&b| b >= 1 && b <= 3) {
            &s[1..]
        } else {
            s
        }
    }

    let mut in_heading = is_heading_sentinel(text);
    let mut in_bold = false;
    let mut in_italic = false;
    let mut current_font = if in_heading {
        fonts.ui_bold
    } else {
        fonts.body
    };
    let mut current_px = if in_heading { heading_font_px } else { font_px };
    let mut line_h = line_height(current_font, current_px) + 4;
    let mut baseline = cy + pad_y + line_height(current_font, current_px);
    let mut remaining = strip_heading_sentinel(text);

    while !remaining.is_empty() && baseline < cy + ch - pad_y {
        let (line, rest, new_bold, new_italic) = next_ttf_line(
            fonts,
            remaining,
            max_px,
            font_px,
            heading_font_px,
            in_heading,
            in_bold,
            in_italic,
        );

        // Draw the line in segments, switching fonts at inline style sentinels.
        // Inside headings all text uses the heading font (no inline variants).
        if !line.is_empty() {
            let mut x = cx + pad_x;
            let line_bytes = line.as_bytes();
            let mut seg_start = 0usize;
            let mut seg_bold = in_bold;
            let mut seg_italic = in_italic;

            for (i, &b) in line_bytes.iter().enumerate() {
                if b >= 4 && b <= 7 {
                    // Flush segment before this sentinel.
                    if i > seg_start {
                        let seg = &line[seg_start..i];
                        let font = select_font(fonts, in_heading, seg_bold, seg_italic);
                        x = draw_str(font, seg, x, baseline, current_px, 15, &mut clipped);
                    }
                    match b {
                        4 => seg_bold = true,
                        5 => seg_bold = false,
                        6 => seg_italic = true,
                        7 => seg_italic = false,
                        _ => {}
                    }
                    seg_start = i + 1;
                }
            }
            // Flush the remaining segment after the last sentinel (or the whole
            // line if there were no sentinels).
            if seg_start < line.len() {
                let seg = &line[seg_start..];
                let font = select_font(fonts, in_heading, seg_bold, seg_italic);
                draw_str(font, seg, x, baseline, current_px, 15, &mut clipped);
            }
        }

        remaining = rest;
        in_bold = new_bold;
        in_italic = new_italic;

        if remaining.starts_with('\n') {
            remaining = &remaining[1..];
            baseline += line_h * crate::layout::PARA_GAP_LINES as i32;
            // Reset inline styles at paragraph boundaries.
            in_bold = false;
            in_italic = false;
            in_heading = is_heading_sentinel(remaining);
            remaining = strip_heading_sentinel(remaining);
            current_font = if in_heading {
                fonts.ui_bold
            } else {
                fonts.body
            };
            current_px = if in_heading { heading_font_px } else { font_px };
            line_h = line_height(current_font, current_px) + 4;
        } else {
            baseline += line_h;
        }
    }
}
