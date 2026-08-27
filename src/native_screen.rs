use embedded_graphics_core::geometry::Size;
use crate::appstate::AppState;

pub trait NativeScreen {
    fn resize(&mut self, size: Size);
    fn deep_clean(&mut self, state: &mut AppState);
    fn deep_sleep(&mut self, state: &mut AppState);
    fn refresh(&mut self, state: &mut AppState);
}

