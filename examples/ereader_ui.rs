//! ereader_ui — iris-ui e-reader layout with header, content, and footer
//!
//! Run in simulator:  cargo sim --example ereader_ui
//! Run on device:     cargo esp-run --example ereader_ui

#![cfg_attr(feature = "esp", no_std)]
#![cfg_attr(feature = "esp", no_main)]

#[cfg(feature = "esp")]
#[macro_use]
extern crate alloc;

use embedded_graphics::mono_font::ascii::{FONT_6X10, FONT_9X15, FONT_9X15_BOLD, FONT_10X20};
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::RgbColor;
use iris_ui::button::make_button;
use iris_ui::device::EmbeddedDrawingContext;
use iris_ui::geom::{Bounds, Insets, Point as GPoint, Size};
use iris_ui::gfx::TextStyle;
use iris_ui::label::make_label;
use iris_ui::layouts::{layout_hbox, layout_std_panel, layout_vbox};
use iris_ui::scene::{click_at, draw_scene, layout_scene, Scene};
use iris_ui::toggle_group::make_toggle_group;
use iris_ui::view::{Align, Flex, View, ViewId};
use iris_ui::{Action, Callback, DrawEvent, GuiEvent, LayoutEvent, Theme};

const SCREEN_W: i32 = 540;
const SCREEN_H: i32 = 960;

const DIALOG_W: i32 = 420;
const DIALOG_H: i32 = 340;
const DIALOG_PAD: i32 = 16;

const BOOK_TEXT: &str = "My dear fellow, said Sherlock Holmes as we sat on \
either side of the fire in his lodgings at Baker Street, life is infinitely \
stranger than anything which the mind of man could invent. We would not dare \
to conceive the things which are really mere commonplaces of existence. If we \
could fly out of that window hand in hand, hover over this great city, gently \
remove the roofs, and peep in at the queer things which are going on, the \
strange coincidences, the plannings, the cross-purposes, the wonderful chains \
of events, working through generations, and leading to the most outré results, \
it would make all fiction with its conventionalities and foreseen conclusions \
most stale and unprofitable. And yet I am not convinced of it, said I. The \
cases which come to light in the papers are, as a rule, bald enough, and vulgar \
enough. We have in our police reports realism pushed to its extreme limits, and \
yet the result is, it must be confessed, neither fascinating nor artistic. A \
certain selection and discretion must be used in producing a realistic effect, \
remarked Holmes. This is wanting in the police report, where more stress is \
laid, perhaps, upon the platitudes of the magistrate than upon the details, \
which to an observer contain the vital essence of the whole matter. Depend \
upon it, there is nothing so unnatural as the commonplace.";

fn make_theme() -> Theme {
    Theme {
        bg: Rgb565::WHITE,
        fg: Rgb565::BLACK,
        selected_bg: Rgb565::BLUE,
        selected_fg: Rgb565::WHITE,
        panel_bg: Rgb565::WHITE,
        font: FONT_9X15,
        bold_font: FONT_9X15_BOLD,
    }
}

fn draw_topbar(e: &mut DrawEvent) {
    e.ctx.fill_rect(&e.view.bounds, &e.theme.panel_bg);
    let b = e.view.bounds;
    let bottom_y = b.position.y + b.size.h - 1;
    e.ctx.line(
        &GPoint::new(b.position.x, bottom_y),
        &GPoint::new(b.position.x + b.size.w, bottom_y),
        &e.theme.fg,
    );
}

fn draw_bottombar(e: &mut DrawEvent) {
    e.ctx.fill_rect(&e.view.bounds, &e.theme.panel_bg);
    let b = e.view.bounds;
    e.ctx.line(
        &GPoint::new(b.position.x, b.position.y),
        &GPoint::new(b.position.x + b.size.w, b.position.y),
        &e.theme.fg,
    );
}

/// Returns the next word-wrapped line and the remaining text.
fn next_line<'a>(text: &'a str, max_chars: usize) -> (&'a str, &'a str) {
    if text.len() <= max_chars {
        return (text.trim_end(), "");
    }
    let cut = &text[..max_chars];
    let break_at = cut.rfind(' ').unwrap_or(max_chars);
    (text[..break_at].trim_end(), text[break_at..].trim_start())
}

fn draw_content(e: &mut DrawEvent) {
    e.ctx.fill_rect(&e.view.bounds, &e.theme.bg);

    let char_w = (e.theme.font.character_size.width + e.theme.font.character_spacing) as i32;
    let char_h = e.theme.font.character_size.height as i32;
    let pad_x = 16i32;
    let pad_y = 12i32;
    let usable_w = e.view.bounds.size.w - pad_x * 2;
    let max_chars = (usable_w / char_w) as usize;

    let style = TextStyle::new(&e.theme.font, &e.theme.fg);
    let x = e.view.bounds.position.x + pad_x;
    let mut y = e.view.bounds.position.y + pad_y;
    let max_y = e.view.bounds.position.y + e.view.bounds.size.h;

    let mut remaining = BOOK_TEXT;
    while !remaining.is_empty() && y + char_h <= max_y {
        let (line, rest) = next_line(remaining, max_chars);
        if !line.is_empty() {
            e.ctx.fill_text(&Bounds::new(x, y, usable_w, char_h), line, &style);
        }
        remaining = rest;
        y += char_h + 3;
    }
}

fn draw_dialog(e: &mut DrawEvent) {
    let b = e.view.bounds;
    // Clear the dialog area to white before children draw on top.
    // iris-ui calls this draw fn before drawing children, so this acts as a
    // background fill that erases whatever content sits behind the dialog.
    e.ctx.fill_rect(&b, &Rgb565::WHITE);
    e.ctx.stroke_rect(&b, &e.theme.fg);
    let inner = Bounds::new(b.position.x + 2, b.position.y + 2, b.size.w - 4, b.size.h - 4);
    e.ctx.stroke_rect(&inner, &e.theme.fg);
}

fn layout_dialog(pass: &mut LayoutEvent) {
    let sw = pass.space.w;
    let sh = pass.space.h;
    if let Some(view) = pass.scene.get_view_mut(pass.target) {
        view.bounds.position.x = (sw - DIALOG_W) / 2;
        view.bounds.position.y = (sh - DIALOG_H) / 2;
        view.bounds.size.w = DIALOG_W;
        view.bounds.size.h = DIALOG_H;
    }
    pass.space = Size::new(DIALOG_W, DIALOG_H);
    layout_vbox(pass);
}

fn handle_click(event: &mut GuiEvent) {
    if event.target == &ViewId::new("settings") {
        info!("showing the dialog");
        event.scene.show_view(&ViewId::new("dialog"));
        event.scene.mark_dirty_all();
    } else if event.target == &ViewId::new("dialog_close") {
        info!("hiding the dialog");
        event.scene.hide_view(&ViewId::new("dialog"));
        event.scene.mark_dirty_all();
    }
}

fn make_scene(w: i32, h: i32) -> Scene {
    let mut scene = Scene::new_with_bounds(Bounds::new(0, 0, w, h));

    // ── Top bar ──────────────────────────────────────────────────────────────
    let topbar_id = ViewId::new("topbar");
    scene.add_view_to_parent(
        make_button(&ViewId::new("settings"), "Settings"),
        &topbar_id,
    );
    scene.add_view_to_parent(make_label("time", "--:-- --"), &topbar_id);
    scene.add_view_to_parent(make_label("battery", "85%"), &topbar_id);
    scene.add_view_to_parent(make_label("booktitle", "Sherlock Holmes"), &topbar_id);

    // ── Content ──────────────────────────────────────────────────────────────
    let content = View {
        name: ViewId::new("content"),
        h_flex: Flex::Resize,
        h_align: Align::Start,
        v_flex: Flex::Resize,
        v_align: Align::Center,
        layout: Some(layout_std_panel),
        draw: Some(draw_content),
        ..Default::default()
    };

    // ── Bottom bar ───────────────────────────────────────────────────────────
    let bottombar_id = ViewId::new("bottombar");
    scene.add_view_to_parent(
        make_label("chapter", "Chapter 3: A Case of Identity"),
        &bottombar_id,
    );
    scene.add_view_to_parent(make_label("page", "Page 42 of 185"), &bottombar_id);

    // ── Root panel (vbox) ────────────────────────────────────────────────────
    let main_id = ViewId::new("main");
    scene.add_view_to_parent(
        View {
            name: topbar_id,
            h_flex: Flex::Resize,
            v_flex: Flex::Intrinsic,
            layout: Some(layout_hbox),
            padding: Insets::new(4, 8, 4, 8),
            draw: Some(draw_topbar),
            ..Default::default()
        },
        &main_id,
    );
    scene.add_view_to_parent(content, &main_id);
    scene.add_view_to_parent(
        View {
            name: bottombar_id,
            h_flex: Flex::Resize,
            v_flex: Flex::Intrinsic,
            layout: Some(layout_hbox),
            padding: Insets::new(4, 8, 4, 8),
            draw: Some(draw_bottombar),
            ..Default::default()
        },
        &main_id,
    );

    // ── Settings dialog (hidden, drawn last so it appears on top) ────────────
    let dialog_id = ViewId::new("dialog");
    scene.add_view_to_parent(make_label("dlg_title", "Settings"), &dialog_id);
    scene.add_view_to_parent(make_label("dlg_font_lbl", "Font Size"), &dialog_id);
    scene.add_view_to_parent(
        make_toggle_group(&ViewId::new("font_size"), vec!["Small", "Medium", "Large"], 1),
        &dialog_id,
    );
    scene.add_view_to_parent(make_label("dlg_bl_lbl", "Backlight"), &dialog_id);
    scene.add_view_to_parent(
        make_toggle_group(&ViewId::new("backlight"), vec!["Off", "Low", "High"], 2),
        &dialog_id,
    );
    scene.add_view_to_parent(make_label("dlg_orient_lbl", "Orientation"), &dialog_id);
    scene.add_view_to_parent(
        make_toggle_group(
            &ViewId::new("orientation"),
            vec!["Port", "Land", "R.Port", "R.Land"],
            0,
        ),
        &dialog_id,
    );
    scene.add_view_to_parent(make_button(&ViewId::new("sync_time"), "Sync Time"), &dialog_id);
    scene.add_view_to_parent(make_label("dlg_battery", "Battery: 85%  (Charging)"), &dialog_id);
    scene.add_view_to_parent(make_button(&ViewId::new("dialog_close"), "Close"), &dialog_id);

    scene.add_view_to_root(View {
        name: main_id,
        h_flex: Flex::Resize,
        v_flex: Flex::Resize,
        layout: Some(layout_vbox),
        bounds: Bounds::new(0, 0, w, h),
        ..Default::default()
    });

    scene.add_view_to_root(View {
        name: dialog_id,
        h_flex: Flex::Resize,
        v_flex: Flex::Resize,
        layout: Some(layout_dialog),
        draw: Some(draw_dialog),
        padding: Insets::new_same(DIALOG_PAD),
        visible: false,
        ..Default::default()
    });

    scene.dump();
    log::info!("scene built");
    scene
}

// ── Simulator path ────────────────────────────────────────────────────────────
#[cfg(feature = "simulator")]
fn format_time_utc(unix_secs: u64) -> String {
    let h24 = (unix_secs / 3600) % 24;
    let m = (unix_secs / 60) % 60;
    let (h12, ampm) = if h24 == 0 {
        (12u64, "AM")
    } else if h24 < 12 {
        (h24, "AM")
    } else if h24 == 12 {
        (12u64, "PM")
    } else {
        (h24 - 12, "PM")
    };
    format!("{}:{:02} {}", h12, m, ampm)
}

#[cfg(feature = "simulator")]
fn main() {
    use embedded_graphics::geometry::Size;
    use embedded_graphics_simulator::{
        OutputSettingsBuilder, SimulatorDisplay, SimulatorEvent, Window,
    };

    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let mut win_w = SCREEN_W;
    let mut win_h = SCREEN_H;

    let mut display: SimulatorDisplay<Rgb565> =
        SimulatorDisplay::new(Size::new(win_w as u32, win_h as u32));
    let settings = OutputSettingsBuilder::new().scale(1).build();
    let mut window = Window::new("ereader_ui", &settings);

    let mut scene = make_scene(win_w, win_h);
    let mut theme = make_theme();
    let handlers: Vec<Callback> = vec![handle_click];
    let mut localtime: u64 = 0;

    'running: loop {
        {
            let mut ctx = EmbeddedDrawingContext::new(&mut display);
            ctx.clip = scene.dirty_rect.clone();
            layout_scene(&mut scene, &theme);
            draw_scene(&mut scene, &mut ctx, &theme);
        }
        window.update(&display);

        let events: Vec<_> = window.events().collect();
        for event in events {
            match event {
                SimulatorEvent::Quit => break 'running,
                SimulatorEvent::MouseButtonUp { point, .. } => {
                    if let Some((target, action)) =
                        click_at(&mut scene, &handlers, GPoint::new(point.x, point.y))
                    {
                        if let Action::Command(ref cmd) = action {
                            if target == ViewId::new("orientation") {
                                let (new_w, new_h) = match cmd.as_str() {
                                    "Land" | "R.Land" => (SCREEN_H, SCREEN_W),
                                    _ => (SCREEN_W, SCREEN_H),
                                };
                                if new_w != win_w || new_h != win_h {
                                    win_w = new_w;
                                    win_h = new_h;
                                    scene.bounds = Bounds::new(0, 0, win_w, win_h);
                                    scene.mark_layout_dirty();
                                    display = SimulatorDisplay::new(
                                        Size::new(win_w as u32, win_h as u32),
                                    );
                                    window = Window::new("ereader_ui", &settings);
                                }
                            } else if target == ViewId::new("font_size") {
                                let (new_font, new_bold) = match cmd.as_str() {
                                    "Small" => (FONT_6X10, FONT_6X10),
                                    "Large" => (FONT_10X20, FONT_10X20),
                                    _ => (FONT_9X15, FONT_9X15_BOLD),
                                };
                                theme.font = new_font;
                                theme.bold_font = new_bold;
                                scene.mark_layout_dirty();
                            }
                        }
                        if target == ViewId::new("sync_time") {
                            use std::time::{SystemTime, UNIX_EPOCH};
                            localtime = SystemTime::now()
                                .duration_since(UNIX_EPOCH)
                                .unwrap()
                                .as_secs();
                            if let Some(view) = scene.get_view_mut(&ViewId::new("time")) {
                                view.title = format_time_utc(localtime);
                            }
                            scene.mark_layout_dirty();
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

// ── ESP path ──────────────────────────────────────────────────────────────────
#[cfg(feature = "esp")]
use esp_backtrace as _;

#[cfg(feature = "esp")]
esp_bootloader_esp_idf::esp_app_desc!();

#[cfg(feature = "esp")]
use ereader::driver::display::{Display, DrawMode};
#[cfg(feature = "esp")]
use ereader::driver::Gt911;
#[cfg(feature = "esp")]
use ereader::driver::gt911::GT911_ADDR_PRIMARY;

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
            let r = pix.1.r() as u32;
            let g = pix.1.g() as u32;
            let b = pix.1.b() as u32;
            let r8 = (r << 3) | (r >> 2);
            let g8 = (g << 2) | (g >> 4);
            let b8 = (b << 3) | (b >> 2);
            let luma8 = (77 * r8 + 150 * g8 + 29 * b8) >> 8;
            let gray4 = (luma8 >> 4) as u8;
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
use log::info;

#[cfg(feature = "esp")]
#[main]
fn main() -> ! {
    use esp_hal::delay::Delay;
    use esp_println::println;

    esp_println::logger::init_logger(log::LevelFilter::Info);

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

    let touch_addr = display.detect_touch_addr().unwrap_or_else(|| {
        log::warn!("GT911 not found; defaulting to primary address");
        GT911_ADDR_PRIMARY
    });
    let mut gt911 = Gt911::new(touch_addr);
    display.configure_touch(&mut gt911, 960, 540);
    delay.delay_millis(200);
    display.init_touch(&mut gt911);

    display.fill(0x0F).unwrap();
    display.flush(DrawMode::WhiteOnBlack).unwrap();
    println!("ereader_ui: display ready");

    let mut bridge = Rgb565ToGray4::new(display);
    let mut scene = make_scene(SCREEN_W, SCREEN_H);
    let mut theme = make_theme();
    let handlers = vec![handle_click as Callback];
    let mut was_touching = false;

    loop {
        let dirty_rect = scene.dirty_rect.clone();
        let was_dirty = !dirty_rect.is_empty();
        let needs_full_refresh = dirty_rect.size.w >= SCREEN_W && dirty_rect.size.h >= SCREEN_H;

        if was_dirty {
            if needs_full_refresh {
                // Ghost-clear pass: needed for dark→light pixel transitions (e.g. dialog dismiss).
                // Matches the page-turn pattern in ereader_full: fill white → WhiteOnBlack → draw → BlackOnWhite.
                bridge.display.fill(0x0F).unwrap();
                bridge.display.flush(DrawMode::WhiteOnBlack).unwrap();
            }
            {
                let mut ctx = EmbeddedDrawingContext::new(&mut bridge);
                ctx.clip = dirty_rect;
                layout_scene(&mut scene, &theme);
                draw_scene(&mut scene, &mut ctx, &theme);
            }
            bridge.flush();
        }

        if let Some((tx, ty)) = bridge.display.read_touch(&mut gt911) {
            if !was_touching {
                // Physical (960×540) → logical portrait (540×960):
                //   draw maps logical (lx,ly) → physical (ly, 539−lx)
                //   so inverse: lx = 539−ty, ly = tx
                let lx = (Display::HEIGHT as i32 - 1) - ty as i32;
                let ly = tx as i32;
                if let Some((target, action)) =
                    click_at(&mut scene, &handlers, GPoint::new(lx, ly))
                {
                    if let Action::Command(ref cmd) = action {
                        if target == ViewId::new("font_size") {
                            let (new_font, new_bold) = match cmd.as_str() {
                                "Small" => (FONT_6X10, FONT_6X10),
                                "Large" => (FONT_10X20, FONT_10X20),
                                _ => (FONT_9X15, FONT_9X15_BOLD),
                            };
                            theme.font = new_font;
                            theme.bold_font = new_bold;
                            scene.mark_layout_dirty();
                        }
                    }
                }
            }
            was_touching = true;
        } else {
            was_touching = false;
        }

        delay.delay_millis(50);
    }
}
