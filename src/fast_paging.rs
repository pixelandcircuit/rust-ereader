#[cfg(feature = "esp")]
use alloc::format;
use crate::appstate::AppState;
use crate::hardware::HardwareAccess;
// use alloc::format;

#[cfg(feature = "esp")]
use embassy_time::{ Instant};

#[cfg(feature = "simulator")]
use std::time::Instant;

use embedded_graphics_core::pixelcolor::Rgb565;
use iris_ui::scene::Scene;
use iris_ui::view::ViewId;
use log::info;


pub const FAST_SCROLL_PANEL_ID: ViewId = ViewId::new("fast_scroll_panel");
pub const FAST_SCROLL_LABEL_ID: ViewId = ViewId::new("fast_scroll_label");

pub struct FastPaging {
    pub fs_active: bool,
    pub fs_target: usize,
    pub fs_last_step: Instant,
    pub fs_pressed_at: Option<Instant>,
    pub forward: bool,
}

impl Default for FastPaging {
    fn default() -> Self {
        FastPaging {
            fs_active: false,
            fs_target: 0usize,
            fs_last_step: Instant::now(),
            fs_pressed_at: None,
            forward: false,
        }
    }
}

impl FastPaging {
    pub fn start_backward(&mut self) {
        self.forward = false;
        self.fs_pressed_at = Some(Instant::now());
    }
    pub fn start_forward(&mut self) {
        self.forward = true;
        self.fs_pressed_at = Some(Instant::now());
    }
    pub fn end(&mut self, state: &mut AppState, hw: &mut dyn HardwareAccess) {
        if self.fs_pressed_at.is_some() {
            if self.fs_active {
                state.session.reader.go_to_page(self.fs_target);
                state.update_content(hw);
                state.scene.hide_view(&FAST_SCROLL_PANEL_ID);
                info!("marking dirty all after hiding the fast scroll panel");
                state.scene.mark_dirty_all();
            } else {
                if self.forward {
                    state.nav_next_page(hw);
                } else {
                    state.nav_prev_page(hw);
                }
            }
        }
        self.fs_active = false;
        self.fs_pressed_at = None;
    }
    pub fn cancel(&mut self) {
        self.fs_active = false;
        self.fs_pressed_at = None;
    }
    pub fn handle_update_label(&mut self, state: &mut AppState) {
        if let Some(fs_pressed_at) = self.fs_pressed_at {
            if !self.fs_active && fs_pressed_at.elapsed().as_millis() >= 1000 {
                self.fs_active = true;
                self.fs_target = state.session.reader.current_page;
                self.fs_last_step = Instant::now();
                update_fast_scroll_label(
                    &mut state.scene,
                    state.session.chapter_idx,
                    state.session.chapter_count(),
                    self.fs_target,
                    state.session.reader.page_count(),
                );
                state.scene.show_view(&FAST_SCROLL_PANEL_ID);
                info!("marking layout dirty after showing fast scroll panel");
                state.scene.mark_layout_dirty();
            }
        }

        if self.fs_active && self.fs_last_step.elapsed().as_millis() >= 200 {
            if self.forward {
                if self.fs_target + 1 >= state.session.reader.page_count() {
                    if state.session.chapter_idx + 1 < state.session.chapter_count() {
                        state
                            .session
                            .go_to_chapter(
                                state.session.chapter_idx + 1,
                                state.book.as_ref(),
                                &state.cfg,
                            )
                            .ok();
                        self.fs_target = 0;
                    }
                } else {
                    self.fs_target += 1;
                }
            } else if self.fs_target == 0 {
                if state.session.chapter_idx > 0 {
                    state
                        .session
                        .go_to_chapter(
                            state.session.chapter_idx - 1,
                            state.book.as_ref(),
                            &state.cfg,
                        )
                        .ok();
                    self.fs_target = state.session.reader.page_count().saturating_sub(1);
                }
            } else {
                self.fs_target -= 1;
            }
            self.fs_last_step = Instant::now();
            update_fast_scroll_label(
                &mut state.scene,
                state.session.chapter_idx,
                state.session.chapter_count(),
                self.fs_target,
                state.session.reader.page_count(),
            );
        }
    }
}

fn update_fast_scroll_label(
    scene: &mut Scene<Rgb565>,
    chapter: usize,
    chapter_count: usize,
    page: usize,
    page_count: usize,
) {
    if let Some(v) = scene.get_view_mut(&FAST_SCROLL_LABEL_ID) {
        v.title = format!(
            "Ch {}/{} · Pg {}/{}",
            chapter + 1,
            chapter_count,
            page + 1,
            page_count
        );
    }
    info!("marking fast scroll panel dirty");
    scene.mark_dirty_view(&FAST_SCROLL_PANEL_ID);
}