use crate::appstate::AppState;
use crate::hardware::{FontSize, Orientation};
use embedded_graphics_core::geometry::Size;

pub trait NativeScreen {
    fn set_orientation(
        &mut self,
        orientation: Orientation,
        state: &mut AppState,
        font_size: FontSize,
    );
    fn deep_clean(&mut self, state: &mut AppState);
    fn deep_sleep(&mut self, state: &mut AppState);
    fn refresh(&mut self, state: &mut AppState);
}
