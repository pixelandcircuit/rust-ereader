//! ereader_ui — iris-ui panel with buttons
//!
//! Run in simulator:  cargo sim --example ereader_ui
//! Run on device:     cargo esp-run --example ereader_ui

#![cfg_attr(feature = "esp", no_std)]
#![cfg_attr(feature = "esp", no_main)]

#[cfg(feature = "esp")]
extern crate alloc;

use embedded_graphics::mono_font::ascii::{FONT_9X15, FONT_9X15_BOLD};
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::{RgbColor, WebColors};
use iris_ui::button::make_button;
use iris_ui::device::EmbeddedDrawingContext;
use iris_ui::geom::{Bounds, Point as GPoint};
use iris_ui::layouts::layout_vbox;
use iris_ui::panel::draw_std_panel;
use iris_ui::scene::{click_at, draw_scene, layout_scene, Scene};
use iris_ui::view::{Flex, View, ViewId};
use iris_ui::Theme;

const SCREEN_W: i32 = 540;
const SCREEN_H: i32 = 960;

fn make_theme() -> Theme {
    Theme {
        bg: Rgb565::WHITE,
        fg: Rgb565::BLACK,
        selected_bg: Rgb565::BLUE,
        selected_fg: Rgb565::WHITE,
        panel_bg: Rgb565::CSS_LIGHT_GRAY,
        font: FONT_9X15,
        bold_font: FONT_9X15_BOLD,
    }
}

fn make_scene() -> Scene {
    let mut scene = Scene::new_with_bounds(Bounds::new(0, 0, SCREEN_W, SCREEN_H));

    let panel = View {
        name: ViewId::new("panel"),
        draw: Some(draw_std_panel),
        h_flex: Flex::Resize,
        v_flex: Flex::Resize,
        layout: Some(layout_vbox),
        bounds: Bounds::new(0, 0, SCREEN_W, SCREEN_H),
        ..Default::default()
    };
    scene.add_view_to_parent(make_button(&ViewId::new("prev"),     "< Previous"), &panel.name);
    scene.add_view_to_parent(make_button(&ViewId::new("next"),     "Next >"),     &panel.name);
    scene.add_view_to_parent(make_button(&ViewId::new("settings"), "Settings"),   &panel.name);
    scene.add_view_to_root(panel);
    scene
}

// ── Simulator path ────────────────────────────────────────────────────────────
#[cfg(feature = "simulator")]
fn main() {
    use embedded_graphics::geometry::Size;
    use embedded_graphics_simulator::{
        OutputSettingsBuilder, SimulatorDisplay, SimulatorEvent, Window,
    };

    let mut display: SimulatorDisplay<Rgb565> =
        SimulatorDisplay::new(Size::new(SCREEN_W as u32, SCREEN_H as u32));
    let settings = OutputSettingsBuilder::new().scale(1).build();
    let mut window = Window::new("ereader_ui", &settings);

    let mut scene = make_scene();
    let theme = make_theme();
    let handlers = Vec::new();

    'running: loop {
        {
            let mut ctx = EmbeddedDrawingContext::new(&mut display);
            ctx.clip = scene.dirty_rect.clone();
            layout_scene(&mut scene, &theme);
            draw_scene(&mut scene, &mut ctx, &theme);
        }
        window.update(&display);

        for event in window.events() {
            match event {
                SimulatorEvent::Quit => break 'running,
                SimulatorEvent::MouseButtonUp { point, .. } => {
                    click_at(&mut scene, &handlers, GPoint::new(point.x, point.y));
                }
                _ => {}
            }
        }
    }
}

// ── ESP path ──────────────────────────────────────────────────────────────────
#[cfg(feature = "esp")]
use ereader::driver::display::{Display, DrawMode};

/// Wraps the Gray4 e-paper display and presents an Rgb565 DrawTarget for iris-ui.
/// Converts luminance to 4-bit gray and rotates coordinates 90° CCW so that
/// portrait (540×960) logical space maps to the physical landscape (960×540) panel.
#[cfg(feature = "esp")]
struct Rgb565ToGray4<'a> {
    display: Display<'a>,
}

#[cfg(feature = "esp")]
impl<'a> Rgb565ToGray4<'a> {
    fn new(display: Display<'a>) -> Self {
        Self { display }
    }
    fn flush(&mut self) {
        self.display.flush(DrawMode::BlackOnWhite).unwrap();
    }
}

#[cfg(feature = "esp")]
impl<'a> embedded_graphics::draw_target::DrawTarget for Rgb565ToGray4<'a> {
    type Color = Rgb565;
    type Error = ();

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = embedded_graphics::Pixel<Self::Color>>,
    {
        for pix in pixels {
            let r = pix.1.r() as u32; // 5-bit (0-31)
            let g = pix.1.g() as u32; // 6-bit (0-63)
            let b = pix.1.b() as u32; // 5-bit (0-31)
            // Expand to 8-bit
            let r8 = (r << 3) | (r >> 2);
            let g8 = (g << 2) | (g >> 4);
            let b8 = (b << 3) | (b >> 2);
            // BT.601 luminance → 4-bit gray (0=black, 15=white)
            let luma8 = (77 * r8 + 150 * g8 + 29 * b8) >> 8;
            let gray4 = (luma8 >> 4) as u8;
            // 90° CCW rotation: portrait (lx, ly) → landscape (ly, HEIGHT-1-lx)
            let px = pix.0.y as u16;
            let py = Display::HEIGHT - 1 - pix.0.x as u16;
            let _ = self.display.set_pixel(px, py, gray4);
        }
        Ok(())
    }
}

#[cfg(feature = "esp")]
impl<'a> embedded_graphics::geometry::OriginDimensions for Rgb565ToGray4<'a> {
    fn size(&self) -> embedded_graphics::geometry::Size {
        embedded_graphics::geometry::Size::new(SCREEN_W as u32, SCREEN_H as u32)
    }
}

#[cfg(feature = "esp")]
use esp_hal::main;

#[cfg(feature = "esp")]
#[main]
fn main() -> ! {
    use esp_hal::delay::Delay;
    use esp_println::println;

    esp_println::logger::init_logger_from_env();

    let config = esp_hal::Config::default()
        .with_cpu_clock(esp_hal::clock::CpuClock::_240MHz);
    let peripherals = esp_hal::init(config);

    esp_alloc::psram_allocator!(
        peripherals.PSRAM,
        esp_hal::psram,
        esp_hal::psram::PsramConfig {
            mode: esp_hal::psram::PsramMode::OctalSpi,
            ..Default::default()
        }
    );

    let delay = Delay::new();

    let mut display = Display::new(
        ereader::pin_config!(peripherals),
        peripherals.DMA_CH0,
        peripherals.LCD_CAM,
        peripherals.RMT,
        peripherals.I2C0,
    )
    .expect("display init");

    delay.delay_millis(100);
    display.power_on();
    delay.delay_millis(10);

    display.fill(0x0F).unwrap();
    display.flush(DrawMode::BlackOnWhite).unwrap();
    println!("ereader_ui: display ready");

    let mut bridge = Rgb565ToGray4::new(display);
    let mut scene = make_scene();
    let theme = make_theme();

    loop {
        let was_dirty = !scene.dirty_rect.is_empty();
        {
            let mut ctx = EmbeddedDrawingContext::new(&mut bridge);
            ctx.clip = scene.dirty_rect.clone();
            layout_scene(&mut scene, &theme);
            draw_scene(&mut scene, &mut ctx, &theme);
        }
        if was_dirty {
            bridge.flush();
        }
        delay.delay_millis(50);
    }
}
