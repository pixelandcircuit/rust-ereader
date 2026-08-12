#[cfg(feature = "esp")]
extern crate alloc;

#[cfg(feature = "esp")]
use alloc::{boxed::Box, string::String, vec::Vec};

use crate::book::{Book, HtmlBook, TxtBook};
use crate::bookview::{layout_cfg, update_content, CONTENT_ID};
use crate::epub::EpubArchive;
use crate::font::{font_px_for, AppFonts};
use crate::hardware::{FontSize, HardwareAccess};
use crate::layout::LayoutConfig;
use crate::reader::BookSession;
#[cfg(feature = "esp")]
use embassy_time::{with_timeout, Duration, Instant, Timer as EmbassyTimer};
use iris_ui::scene::{layout_scene, Scene};
use iris_ui::Theme;
#[cfg(feature = "simulator")]
use std::time::Instant;

pub struct AppState {
    pub partial_refresh_count: u32,
    /// Counts full-screen page turns since the last full-quality (15-frame) refresh.
    /// Resets to 0 when a full-quality pass runs.
    pub full_quality_count: u32,
    pub current_filename: String,
    pub last_interaction: Instant,
    pub cfg: LayoutConfig,
    pub book: Box<dyn Book>,
    pub session: BookSession,
    pub scene: Scene,
    pub fonts: AppFonts,
    pub theme: Theme,
}

impl AppState {
    pub fn update_content(&mut self, hw: &dyn HardwareAccess) {
        update_content(&mut self.scene, &self.session, font_px_for(hw.font_size()));
    }

    pub fn nav_prev_page(&mut self, hw: &mut dyn HardwareAccess) {
        if self.session.reader.current_page == 0 {
            self.session.prev_chapter(&*self.book, &self.cfg).ok();
        } else {
            self.session.reader.turn_page(false);
        }
        self.update_content(hw);
    }
    pub fn nav_next_page(&mut self, hw: &mut dyn HardwareAccess) {
        if self.session.reader.current_page + 1 >= self.session.reader.page_count() {
            self.session.next_chapter(&*self.book, &self.cfg).ok();
        } else {
            self.session.reader.turn_page(true);
        }
        self.update_content(hw);
    }
}

pub fn book_from_data(filename: &str, data: Vec<u8>) -> Box<dyn Book> {
    let lower = filename.to_ascii_lowercase();
    if lower.ends_with(".html") || lower.ends_with(".htm") {
        Box::new(HtmlBook::from_vec(data))
    } else if lower.ends_with(".txt") {
        Box::new(TxtBook::from_vec(data))
    } else {
        match EpubArchive::from_vec(data) {
            Ok(epub) => Box::new(epub),
            Err(e) => {
                log::warn!("failed to open epub {}: {:?}", filename, e);
                Box::new(TxtBook::from_vec(b"[Could not open file]".to_vec()))
            }
        }
    }
}

/// Run a layout pass and build a LayoutConfig from the real content view bounds.
/// Must be called whenever the scene size or UI font changes.
pub fn cfg_from_scene(
    scene: &mut Scene,
    theme: &Theme,
    fonts: &AppFonts,
    font_size: FontSize,
) -> LayoutConfig {
    layout_scene(scene, theme);
    let bounds = scene
        .get_view_bounds(&CONTENT_ID)
        .expect("content view not in scene");
    layout_cfg(fonts, font_size, bounds.size.w, bounds.size.h)
}
