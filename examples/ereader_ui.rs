//! ereader_ui — iris-ui e-reader layout with header, content, and footer
//!
//! Run in simulator:  cargo sim --example ereader_ui
//! Run on device:     cargo esp-run --example ereader_ui

#![cfg_attr(feature = "esp", no_std)]
#![cfg_attr(feature = "esp", no_main)]

#[cfg(feature = "esp")]
#[macro_use]
extern crate alloc;
#[cfg(feature = "esp")]
use alloc::string::String;

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
use ereader::hardware::{BacklightLevel, FontSize, HardwareAccess, Orientation};
#[cfg(feature = "simulator")]
use ereader::hardware::SimHardware;
#[cfg(feature = "esp")]
use ereader::hardware::EspHardware;

const DIALOG_W: i32 = 420;
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

const DIALOG_ID:ViewId = ViewId::new("dialog");


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
    // Give children unconstrained vertical space so layout_vbox can measure them.
    if let Some(view) = pass.scene.get_view_mut(pass.target) {
        view.bounds.size.w = DIALOG_W;
        view.bounds.size.h = 4000;
    }
    pass.space = Size::new(DIALOG_W, 4000);
    layout_vbox(pass);
    // Measure the actual height used by children (positions are in dialog-local space).
    let mut content_bottom = DIALOG_PAD;
    for kid in pass.scene.get_children_ids(&DIALOG_ID) {
        if let Some(child) = pass.scene.get_view(&kid) {
            content_bottom = content_bottom.max(child.bounds.position.y + child.bounds.size.h);
        }
    }
    let dialog_h = content_bottom + DIALOG_PAD;
    // Center the dialog using the measured height.
    if let Some(view) = pass.scene.get_view_mut(pass.target) {
        view.bounds.position.x = (sw - DIALOG_W) / 2;
        view.bounds.position.y = (sh - dialog_h) / 2;
        view.bounds.size.h = dialog_h;
    }
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
    scene.add_view_to_parent(make_label("dlg_title", "Settings"), &DIALOG_ID);
    scene.add_view_to_parent(make_label("dlg_font_lbl", "Font Size"), &DIALOG_ID);
    scene.add_view_to_parent(
        make_toggle_group(&ViewId::new("font_size"), vec!["Small", "Medium", "Large"], 1),
        &DIALOG_ID,
    );
    scene.add_view_to_parent(make_label("dlg_bl_lbl", "Backlight"), &DIALOG_ID);
    scene.add_view_to_parent(
        make_toggle_group(&ViewId::new("backlight"), vec!["Off", "Low", "High"], 2),
        &DIALOG_ID,
    );
    scene.add_view_to_parent(make_label("dlg_orient_lbl", "Orientation"), &DIALOG_ID);
    scene.add_view_to_parent(
        make_toggle_group(
            &ViewId::new("orientation"),
            vec!["Port", "Land", "R.Port", "R.Land"],
            0,
        ),
        &DIALOG_ID,
    );
    scene.add_view_to_parent(make_button(&ViewId::new("sync_time"), "Sync Time"), &DIALOG_ID);
    scene.add_view_to_parent(make_label("dlg_battery", "Battery: 85%  (Charging)"), &DIALOG_ID);
    scene.add_view_to_parent(make_button(&ViewId::new("dialog_close"), "Close"), &DIALOG_ID);

    scene.add_view_to_root(View {
        name: main_id,
        h_flex: Flex::Resize,
        v_flex: Flex::Resize,
        layout: Some(layout_vbox),
        bounds: Bounds::new(0, 0, w, h),
        ..Default::default()
    });

    scene.add_view_to_root(View {
        name: DIALOG_ID,
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

    let mut hw = SimHardware::new();
    let (mut win_w, mut win_h) = hw.orientation().logical_size();

    let mut display: SimulatorDisplay<Rgb565> =
        SimulatorDisplay::new(Size::new(win_w as u32, win_h as u32));
    let settings = OutputSettingsBuilder::new().scale(1).build();
    let mut window = Window::new("ereader_ui", &settings);

    let mut scene = make_scene(win_w, win_h);
    let mut theme = make_theme();
    let handlers: Vec<Callback> = vec![handle_click];

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
                                hw.set_orientation(Orientation::from_cmd(cmd.as_str()));
                                let (new_w, new_h) = hw.orientation().logical_size();
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
                                hw.set_font_size(FontSize::from_cmd(cmd.as_str()));
                                (theme.font, theme.bold_font) = match hw.font_size() {
                                    FontSize::Small  => (FONT_6X10,  FONT_6X10),
                                    FontSize::Medium => (FONT_9X15,  FONT_9X15_BOLD),
                                    FontSize::Large  => (FONT_10X20, FONT_10X20),
                                };
                                scene.mark_layout_dirty();
                            } else if target == ViewId::new("backlight") {
                                hw.set_backlight_level(BacklightLevel::from_cmd(cmd.as_str()));
                            }
                        }
                        if target == ViewId::new("sync_time") {
                            let t = hw.current_time_secs();
                            if let Some(view) = scene.get_view_mut(&ViewId::new("time")) {
                                view.title = format_time_utc(t);
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

// ── Flash storage ─────────────────────────────────────────────────────────────
#[cfg(feature = "esp")]
use esp_storage::FlashStorage;
#[cfg(feature = "esp")]
use sequential_storage::{cache::NoCache, map};

#[cfg(feature = "esp")]
struct FlashAdapter(FlashStorage);

#[cfg(feature = "esp")]
impl embedded_storage::nor_flash::ErrorType for FlashAdapter {
    type Error = esp_storage::FlashStorageError;
}

#[cfg(feature = "esp")]
impl embedded_storage_async::nor_flash::ReadNorFlash for FlashAdapter {
    const READ_SIZE: usize = FlashStorage::WORD_SIZE as usize;
    async fn read(&mut self, offset: u32, bytes: &mut [u8]) -> Result<(), Self::Error> {
        embedded_storage::nor_flash::ReadNorFlash::read(&mut self.0, offset, bytes)
    }
    fn capacity(&self) -> usize {
        embedded_storage::nor_flash::ReadNorFlash::capacity(&self.0)
    }
}

#[cfg(feature = "esp")]
impl embedded_storage_async::nor_flash::NorFlash for FlashAdapter {
    const WRITE_SIZE: usize = FlashStorage::WORD_SIZE as usize;
    const ERASE_SIZE: usize = FlashStorage::SECTOR_SIZE as usize;
    async fn erase(&mut self, from: u32, to: u32) -> Result<(), Self::Error> {
        embedded_storage::nor_flash::NorFlash::erase(&mut self.0, from, to)
    }
    async fn write(&mut self, offset: u32, bytes: &[u8]) -> Result<(), Self::Error> {
        embedded_storage::nor_flash::NorFlash::write(&mut self.0, offset, bytes)
    }
}

#[cfg(feature = "esp")]
fn block_on<F: core::future::Future>(mut f: F) -> F::Output {
    use core::{pin::Pin, task::{Context, Poll, RawWaker, RawWakerVTable, Waker}};
    static VTABLE: RawWakerVTable =
        RawWakerVTable::new(|p| RawWaker::new(p, &VTABLE), |_| {}, |_| {}, |_| {});
    let waker = unsafe { Waker::from_raw(RawWaker::new(core::ptr::null(), &VTABLE)) };
    let mut cx = Context::from_waker(&waker);
    loop {
        match unsafe { Pin::new_unchecked(&mut f) }.poll(&mut cx) {
            Poll::Ready(v) => return v,
            Poll::Pending => {}
        }
    }
}

// Keys 10–12 to avoid collisions with ereader_full (which uses 0–4).
#[cfg(feature = "esp")]
const NVS_RANGE: core::ops::Range<u32> = 0x9000..0xF000;
#[cfg(feature = "esp")]
const KEY_FONT: u8 = 10;
#[cfg(feature = "esp")]
const KEY_BL: u8 = 11;
#[cfg(feature = "esp")]
const KEY_ORI: u8 = 12;

/// Returns (font_idx, bl_idx, ori_idx). Defaults: Medium (1), High (2), Portrait (0).
#[cfg(feature = "esp")]
fn load_settings() -> (usize, usize, usize) {
    let mut flash = FlashAdapter(FlashStorage::new());
    let mut cache = NoCache::new();
    let mut buf = [0u8; 64];
    let mut load = |key: u8, default: u32| -> u32 {
        match block_on(map::fetch_item::<u8, u32, _>(
            &mut flash, NVS_RANGE, &mut cache, &mut buf, &key,
        )) {
            Ok(Some(v)) => v,
            _ => default,
        }
    };
    let font = load(KEY_FONT, 1) as usize;
    let bl   = load(KEY_BL,   2) as usize;
    let ori  = load(KEY_ORI,  0) as usize;
    log::info!("settings loaded: font={} bl={} ori={}", font, bl, ori);
    (font, bl, ori)
}

#[cfg(feature = "esp")]
fn save_settings(font_idx: usize, bl_idx: usize, ori_idx: usize) {
    let mut flash = FlashAdapter(FlashStorage::new());
    let mut cache = NoCache::new();
    let mut buf = [0u8; 64];
    let mut save = |key: u8, val: u32| {
        if let Err(e) = block_on(map::store_item::<u8, u32, _>(
            &mut flash, NVS_RANGE, &mut cache, &mut buf, &key, &val,
        )) {
            log::warn!("flash save key {} failed: {:?}", key, e);
        }
    };
    save(KEY_FONT, font_idx as u32);
    save(KEY_BL,   bl_idx   as u32);
    save(KEY_ORI,  ori_idx  as u32);
}

/// Wraps the Gray4 e-paper display and presents an Rgb565 DrawTarget for iris-ui.
/// Converts Rgb565 luminance to 4-bit gray and applies orientation rotation so
/// the logical coordinate space matches what the user sees.
#[cfg(feature = "esp")]
struct Rgb565ToGray4<'a> {
    display:     Display<'a>,
    orientation: Orientation,
}

#[cfg(feature = "esp")]
impl<'a> Rgb565ToGray4<'a> {
    fn new(display: Display<'a>, orientation: Orientation) -> Self {
        Self { display, orientation }
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
            let (px, py) = self.orientation.logical_to_phys(
                pix.0.x as u16, pix.0.y as u16,
            );
            let _ = self.display.set_pixel(px, py, gray4);
        }
        Ok(())
    }
}

#[cfg(feature = "esp")]
impl<'a> embedded_graphics::geometry::OriginDimensions for Rgb565ToGray4<'a> {
    fn size(&self) -> embedded_graphics::geometry::Size {
        let (w, h) = self.orientation.logical_size();
        embedded_graphics::geometry::Size::new(w as u32, h as u32)
    }
}

#[cfg(feature = "esp")]
use esp_hal::{
    interrupt::software::SoftwareInterruptControl,
    ledc::{
        channel::{self, ChannelIFace},
        timer::{self, TimerIFace},
        LSGlobalClkSource, Ledc, LowSpeed,
    },
    rtc_cntl::Rtc,
    time::Rate,
    timer::timg::TimerGroup,
};
#[cfg(feature = "esp")]
use embassy_executor::Spawner;
#[cfg(feature = "esp")]
use embassy_net::{Runner, StackResources, IpEndpoint, IpAddress, Ipv4Address,
                  udp::{PacketMetadata, UdpSocket}};
#[cfg(feature = "esp")]
use embassy_time::{Duration, Timer as EmbassyTimer};
#[cfg(feature = "esp")]
use esp_radio::wifi::{Config, ControllerConfig, Interface, WifiController, sta::StationConfig};
#[cfg(feature = "esp")]
use static_cell::StaticCell;
use log::info;

// WiFi credentials — set WIFI_SSID and WIFI_PASS at build time.
#[cfg(feature = "esp")]
const SSID:     &str = match option_env!("WIFI_SSID") { Some(s) => s, None => "SSID" };
#[cfg(feature = "esp")]
const PASSWORD: &str = match option_env!("WIFI_PASS") { Some(s) => s, None => "PASSWORD" };

#[cfg(feature = "esp")]
const NTP_ADDR:        [u8; 4] = [216, 239, 35, 0]; // time.google.com
#[cfg(feature = "esp")]
const NTP_UNIX_OFFSET: u64     = 2_208_988_800;     // NTP epoch → Unix epoch

#[cfg(feature = "esp")]
macro_rules! mk_static {
    ($t:ty, $val:expr) => {{
        static STATIC_CELL: StaticCell<$t> = StaticCell::new();
        STATIC_CELL.uninit().write(($val))
    }};
}

#[cfg(feature = "esp")]
#[embassy_executor::task]
async fn wifi_connection(mut controller: WifiController<'static>) {
    loop {
        match controller.connect_async().await {
            Ok(_)  => { controller.wait_for_disconnect_async().await.ok(); }
            Err(e) => { log::warn!("wifi connect error: {:?}", e); }
        }
        EmbassyTimer::after(Duration::from_secs(5)).await;
    }
}

#[cfg(feature = "esp")]
#[embassy_executor::task]
async fn net_task(mut runner: Runner<'static, Interface<'static>>) {
    runner.run().await
}

/// Query time.google.com via NTP and return Unix seconds, or None on error.
// #[cfg(feature = "esp")]
// async fn query_ntp(stack: embassy_net::Stack<'static>) -> Option<u64> {
//     let mut rx_meta = [PacketMetadata::EMPTY; 4];
//     let mut rx_buf  = [0u8; 512];
//     let mut tx_meta = [PacketMetadata::EMPTY; 4];
//     let mut tx_buf  = [0u8; 256];
//     let mut socket  = UdpSocket::new(stack, &mut rx_meta, &mut rx_buf, &mut tx_meta, &mut tx_buf);
//     socket.bind(12345).ok()?;
//
//     let endpoint = IpEndpoint::new(IpAddress::Ipv4(Ipv4Address::from_octets(NTP_ADDR)), 123);
//     let mut pkt = [0u8; 48];
//     pkt[0] = 0x1B; // LI=0, VN=3, Mode=3 (client)
//     socket.send_to(&pkt, endpoint).await.ok()?;
//
//     let (n, _) = socket.recv_from(&mut pkt).await.ok()?;
//     if n < 48 { return None; }
//
//     let ntp_secs = u32::from_be_bytes([pkt[40], pkt[41], pkt[42], pkt[43]]) as u64;
//     if ntp_secs <= NTP_UNIX_OFFSET { return None; }
//     Some(ntp_secs - NTP_UNIX_OFFSET)
// }

#[cfg(feature = "esp")]
#[esp_rtos::main]
async fn main(spawner: Spawner) -> ! {
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
    // SRAM heap required by the WiFi stack (must be separate from PSRAM).
    esp_alloc::heap_allocator!(size: 72 * 1024);

    // Must run before any EmbassyTimer use and before esp_radio::wifi::new.
    let timg0  = TimerGroup::new(peripherals.TIMG0);
    let sw_int = SoftwareInterruptControl::new(peripherals.SW_INTERRUPT);
    esp_rtos::start(timg0.timer0, sw_int.software_interrupt0);

    let rtc = Rtc::new(peripherals.LPWR);

    let mut display = Display::new(
        ereader::pin_config!(peripherals),
        peripherals.DMA_CH0,
        peripherals.LCD_CAM,
        peripherals.RMT,
        peripherals.I2C0,
    )
    .expect("display init");

    EmbassyTimer::after(Duration::from_millis(100)).await;
    display.power_on();
    EmbassyTimer::after(Duration::from_millis(10)).await;

    let touch_addr = display.detect_touch_addr().unwrap_or_else(|| {
        log::warn!("GT911 not found; defaulting to primary address");
        GT911_ADDR_PRIMARY
    });
    let mut gt911 = Gt911::new(touch_addr);
    display.configure_touch(&mut gt911, 960, 540);
    EmbassyTimer::after(Duration::from_millis(200)).await;
    display.init_touch(&mut gt911);

    display.fill(0x0F).unwrap();
    display.flush(DrawMode::WhiteOnBlack).unwrap();
    println!("ereader_ui: display ready");

    let mut ledc = Ledc::new(peripherals.LEDC);
    ledc.set_global_slow_clock(LSGlobalClkSource::APBClk);
    let mut lstimer0 = ledc.timer::<LowSpeed>(timer::Number::Timer0);
    lstimer0.configure(timer::config::Config {
        duty:         timer::config::Duty::Duty8Bit,
        clock_source: timer::LSClockSource::APBClk,
        frequency:    Rate::from_khz(1),
    }).unwrap();
    let mut bl_ch = ledc.channel(channel::Number::Channel0, peripherals.GPIO11);
    bl_ch.configure(channel::config::Config {
        timer:      &lstimer0,
        duty_pct:   100,
        drive_mode: esp_hal::gpio::DriveMode::PushPull,
    }).unwrap();
    let (font_idx, bl_idx, ori_idx) = load_settings();
    // Capture seed before rtc is moved into hw.
    let seed = rtc.current_time_us();
    let mut hw = EspHardware::new(
        bl_ch,
        rtc,
        FontSize::from_index(font_idx),
        BacklightLevel::from_index(bl_idx),
        Orientation::from_index(ori_idx),
    );
    let (lw, lh) = hw.orientation().logical_size();
    let mut bridge = Rgb565ToGray4::new(display, hw.orientation());
    let mut scene = make_scene(lw, lh);
    let mut theme = make_theme();
    (theme.font, theme.bold_font) = match hw.font_size() {
        FontSize::Small  => (FONT_6X10,  FONT_6X10),
        FontSize::Medium => (FONT_9X15,  FONT_9X15_BOLD),
        FontSize::Large  => (FONT_10X20, FONT_10X20),
    };
    let handlers = vec![handle_click as Callback];
    let mut was_touching = false;

    // ── WiFi + NTP setup ─────────────────────────────────────────────────────
    let station_config = Config::Station(
        StationConfig::default()
            .with_ssid(SSID)
            .with_password(PASSWORD.into()),
    );
    let (_controller, _interfaces) = esp_radio::wifi::new(
        peripherals.WIFI,
        ControllerConfig::default().with_initial_config(station_config),
    ).expect("wifi init");

    let _ = seed; // used by the commented-out embassy_net setup below
    // let (stack, runner) = embassy_net::new(
    //     _interfaces.station,
    //     embassy_net::Config::dhcpv4(Default::default()),
    //     mk_static!(StackResources<3>, StackResources::<3>::new()),
    //     seed,
    // );
    // spawner.spawn(net_task(runner).expect("net_task"));
    // spawner.spawn(wifi_connection(_controller).expect("wifi_connection"));

    // info!("connecting to wifi...");
    // stack.wait_config_up().await;
    // info!("wifi connected, querying NTP");
    // if let Some(unix_secs) = query_ntp(stack).await {
    //     hw.rtc.set_current_time_us(unix_secs * 1_000_000);
    //     let time_str = format_time_utc(unix_secs);
    //     if let Some(view) = scene.get_view_mut(&ViewId::new("time")) {
    //         view.title = time_str.clone();
    //     }
    //     scene.mark_layout_dirty();
    //     info!("time synced: {}", time_str);
    // } else {
    //     info!("NTP query failed");
    // }

    loop {
        let dirty_rect = scene.dirty_rect.clone();
        let was_dirty = !dirty_rect.is_empty();
        let (scene_w, scene_h) = hw.orientation().logical_size();
        let needs_full_refresh = dirty_rect.size.w >= scene_w && dirty_rect.size.h >= scene_h;

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
                let (lx, ly) = hw.orientation().phys_to_logical(tx, ty);
                if let Some((target, action)) =
                    click_at(&mut scene, &handlers, GPoint::new(lx, ly))
                {
                    if let Action::Command(ref cmd) = action {
                        if target == ViewId::new("font_size") {
                            hw.set_font_size(FontSize::from_cmd(cmd.as_str()));
                            (theme.font, theme.bold_font) = match hw.font_size() {
                                FontSize::Small  => (FONT_6X10,  FONT_6X10),
                                FontSize::Medium => (FONT_9X15,  FONT_9X15_BOLD),
                                FontSize::Large  => (FONT_10X20, FONT_10X20),
                            };
                            scene.mark_layout_dirty();
                            save_settings(hw.font_size().to_index(), hw.backlight_level().to_index(), hw.orientation().to_index());
                        } else if target == ViewId::new("backlight") {
                            hw.set_backlight_level(BacklightLevel::from_cmd(cmd.as_str()));
                            scene.mark_dirty_all();
                            save_settings(hw.font_size().to_index(), hw.backlight_level().to_index(), hw.orientation().to_index());
                        } else if target == ViewId::new("orientation") {
                            hw.set_orientation(Orientation::from_cmd(cmd.as_str()));
                            bridge.orientation = hw.orientation();
                            let (new_w, new_h) = hw.orientation().logical_size();
                            scene.bounds = Bounds::new(0, 0, new_w, new_h);
                            scene.mark_layout_dirty();
                            save_settings(hw.font_size().to_index(), hw.backlight_level().to_index(), hw.orientation().to_index());
                        }
                    }
                    if target == ViewId::new("sync_time") {
                        info!("sync_time pressed, querying NTP");
                        // if let Some(unix_secs) = query_ntp(stack).await {
                        //     let time_str = format_time_utc(unix_secs);
                        //     if let Some(view) = scene.get_view_mut(&ViewId::new("time")) {
                        //         view.title = time_str.clone();
                        //     }
                        //     scene.mark_layout_dirty();
                        //     info!("time synced: {}", time_str);
                        // } else {
                        //     info!("NTP query failed");
                        // }
                    }
                }
            }
            was_touching = true;
        } else {
            was_touching = false;
        }

        EmbassyTimer::after(Duration::from_millis(50)).await;
    }
}
