use embedded_graphics_core::pixelcolor::{Rgb565, RgbColor};
use iris_ui::geom::Bounds;
use iris_ui::gfx::DrawingContext;
use iris_ui::scene::Scene;
use iris_ui::view::ViewId;
use iris_ui::DrawEvent;
use log::info;

pub struct ProgressBarState {
    pub progress: u8, // 0–100
}

pub fn draw_progress_bar(e: &mut DrawEvent<Rgb565>) {
    // e.view.bounds is the view's own bounds in parent-local coordinates,
    // consistent with how draw_book_content uses it. e.bounds is the scene bounds.
    let bounds = e.view.bounds;
    let pct = e
        .view
        .get_state::<ProgressBarState>()
        .map(|s| s.progress)
        .unwrap_or(0);
    e.ctx.fill_rect(&bounds, &Rgb565::WHITE);
    if pct > 0 {
        let fill_w = (bounds.size.w as u32 * pct as u32 / 100) as i32;
        let fill = Bounds::new(bounds.position.x, bounds.position.y, fill_w, bounds.size.h);
        e.ctx.fill_rect(&fill, &Rgb565::BLACK);
    }
    e.ctx.stroke_rect(&bounds, &Rgb565::BLACK);
}

pub const LOADING_PROGRESS_BAR_ID: ViewId = ViewId::new("loading_progress_bar");

pub fn set_loading_progress(scene: &mut Scene<Rgb565>, pct: u8) {
    if let Some(v) = scene.get_view_mut(&LOADING_PROGRESS_BAR_ID) {
        if let Some(s) = v.get_state::<ProgressBarState>() {
            s.progress = pct;
            info!("set loading progress {}", s.progress);
        }
    }
}
