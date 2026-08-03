#[cfg(feature = "esp")]
use alloc::string::String;
use crate::CONTENT_ID;
use embedded_graphics_core::pixelcolor::{Rgb565, RgbColor};
use ereader::font::TextRenderer;
use ereader::reader::BookSession;
use iris_ui::gfx::DrawingContext;
use iris_ui::scene::Scene;
use iris_ui::view::ViewId;
use iris_ui::DrawEvent;

pub struct BookState {
    pub text: String,
    pub font_px: f32,
    pub renderer: TextRenderer,
}

pub fn draw_book_content(e: &mut DrawEvent) {
    let bounds = e.view.bounds;
    // get_state returns &mut BookState which borrows e.view. We need e.ctx in the
    // pixel closure below, but can't hold both borrows simultaneously through e.
    // Extract a raw pointer to renderer (safe: BookState lives in a Box inside the
    // view for the entire function; we never replace the state box during rendering).
    let (text, font_px, renderer_ptr) = match e.view.get_state::<BookState>() {
        Some(s) => (s.text.clone(), s.font_px, &s.renderer as *const TextRenderer),
        None => return,
    };
    let renderer = unsafe { &*renderer_ptr };
    e.ctx.fill_rect(&bounds, &Rgb565::WHITE);
    crate::render_ttf_text(renderer, &text, font_px, bounds, |px, py, g4| {
        let gray8 = (g4 << 4) | g4;
        let v5 = (gray8 >> 3) as u8;
        let v6 = (gray8 >> 2) as u8;
        e.ctx.put_pixel(px, py, &Rgb565::new(v5, v6, v5));
    });
    e.ctx.stroke_rect(&bounds, &Rgb565::BLACK);
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