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
use alloc::{boxed::Box, string::String};
use Align::Center;
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::RgbColor;
use ereader::epub::EpubArchive;
use ereader::font::{char_advance, draw_str, font_px_for, line_height, measure_width};
#[cfg(feature = "esp")]
use ereader::hardware::EspHardware;
#[cfg(feature = "simulator")]
use ereader::hardware::SimHardware;
use ereader::hardware::{BacklightLevel, FontSize, HardwareAccess, Orientation};
use ereader::layout::{FontMetrics, LayoutConfig};
use ereader::reader::BookSession;
use iris_ui::button::{make_button, make_full_button};
use iris_ui::device::EmbeddedDrawingContext;
use iris_ui::geom::{Bounds, Insets, Point as GPoint, Size};
use iris_ui::label::make_label;
use iris_ui::layouts::{layout_hbox, layout_std_panel, layout_vbox};
use iris_ui::scene::{click_at, draw_scene, layout_scene, Scene};
use iris_ui::toggle_group::{make_toggle_group, SelectOneOfState};
use iris_ui::view::{Align, Flex, View, ViewId};
use iris_ui::{util, Callback, DrawEvent, FontKind, GuiEvent, LayoutEvent, Theme, ViewStyle};


const EPUB_DATA: &[u8] = include_bytes!("sherlock_holmes.epub");

const DIALOG_ID:ViewId = ViewId::new("dialog");
const ORIENTATION_ID:ViewId = ViewId::new("orientation");
const BACKLIGHT_ID:ViewId = ViewId::new("backlight");
const FONT_SIZE_ID:ViewId = ViewId::new("font_size");

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

/// Sync the settings dialog toggle groups to reflect the actual loaded settings.
/// make_scene() hardcodes default selections; call this after loading persisted values.
fn sync_settings_ui(scene: &mut Scene, font_idx: usize, bl_idx: usize, ori_idx: usize) {
    for (id, idx) in [
        (FONT_SIZE_ID.clone(),           font_idx),
        (BACKLIGHT_ID,       bl_idx),
        (ORIENTATION_ID,     ori_idx),
    ] {
        if let Some(v) = scene.get_view_mut(&id) {
            if let Some(s) = v.get_state::<SelectOneOfState>() {
                s.selected = idx;
            }
        }
    }
}

fn make_spacer(id: &ViewId) -> View {
    View {
        name: id.clone(),
        h_flex: Shrink,
        h_align: Center,
        v_flex: Shrink,
        v_align: Center,
        bounds: Bounds::new(0,0,10,10),
        visible:true,
        title: "spacer".into(),
        draw:Some(|e| {
            e.ctx.stroke_rect(&e.view.bounds, &e.theme.panel.text);
        }),
        input:None,
        layout: Some(|e|{
            if let Some(view) = e.scene.get_view_mut(&e.target) {
                if view.h_flex == Grow {
                    // view.bounds.size.w = e.space.w;
                }
                if view.v_flex == Grow {
                    // view.bounds.size.h = e.space.h;
                }
            }
        }),
        state: None
    }
}

fn make_scene(font: &'static Font, w: i32, h: i32) -> Scene {
    let mut scene = Scene::new_with_bounds(Bounds::new(0, 0, w, h));
    let main_id = ViewId::new("main");
    let main_panel = make_panel(&main_id)
        .with_layout(Some(layout_vbox))
        .with_h_flex(Flex::Grow)
        .with_v_flex(Flex::Grow)
        .with_state(Some(Box::new(PanelState {
            border_visible: true,
            gap: 0,
            padding: Insets::new_same(0),
        })));

    // ── Top bar ──────────────────────────────────────────────────────────────
    {
        let topbar_id = ViewId::new("topbar");
        let settings_button = make_full_button(&ViewId::new("settings"), "Settings", "settings", false);
        scene.add_view_to_parent(settings_button, &topbar_id);
        scene.add_view_to_parent(make_label("time", "--:-- --"), &topbar_id);
        scene.add_view_to_parent(make_label("battery", "85%"), &topbar_id);
        scene.add_view_to_parent(make_spacer(&ViewId::new("spacer2")).with_h_flex(Grow), &topbar_id);
        scene.add_view_to_parent(make_label("booktitle", "Sherlock Holmes"), &topbar_id);
        let topbar = make_panel(&topbar_id)
            .with_layout(Some(layout_hbox))
            .with_h_flex(Flex::Grow)
            .with_state(Some(Box::new(PanelState {
                border_visible: true,
                gap: 5,
                padding: Insets::new_same(5),
            })));
        scene.add_view_to_parent(topbar, &main_id);
    }


    // content — plain View with BookState; draw_book_content renders TTF text
    {
        fn fill_all_space(layout: &mut LayoutEvent) {
            if let Some(view) = layout.scene.get_view_mut(&layout.target) {
                view.bounds.size.w = layout.space.w;
                view.bounds.size.h = layout.space.h;
            }
        }

        let content = View {
            name: CONTENT_ID.clone(),
            draw: Some(draw_book_content),
            state: Some(Box::new(BookState {
                text: String::new(),
                font_px: 22.0,
                font,
            })),
            ..Default::default()
        }
            .with_layout(Some(fill_all_space))
            .with_v_flex(Flex::Grow)
            .with_h_flex(Flex::Grow)
            .with_v_align(Center)
            .with_h_align(Center);
        scene.add_view_to_parent(content, &main_id);
    }


    // ── Bottom bar ───────────────────────────────────────────────────────────
    {
        let bottombar_id = ViewId::new("bottombar");
        scene.add_view_to_parent(make_button(&ViewId::new("prev_page"), "< Prev"), &bottombar_id);
        scene.add_view_to_parent(make_label("chapter", "Loading..."), &bottombar_id);
        scene.add_view_to_parent(make_label("page", ""), &bottombar_id);
        scene.add_view_to_parent(make_spacer(&ViewId::new("spacer2")).with_h_flex(Grow), &bottombar_id);
        scene.add_view_to_parent(make_button(&ViewId::new("next_page"), "Next >"), &bottombar_id);
        let bottombar = make_panel(&bottombar_id)
            .with_layout(Some(layout_hbox))
            .with_h_flex(Flex::Grow)
            .with_visible(true)
            .with_state(Some(Box::new(PanelState {
                border_visible: true,
                gap: 5,
                padding: Insets::new_same(5),
            })));
        scene.add_view_to_parent(bottombar, &main_id);
    }

    scene.add_view_to_root(main_panel);

    // --- settings dialog --------------------
    {
        let dialog_panel = make_panel(&DIALOG_ID)
            .with_layout(Some(layout_vbox))
            .with_h_flex(Flex::Grow)
            .with_v_flex(Flex::Grow)
            .with_visible(false)
            .with_state(Some(Box::new(PanelState {
                border_visible: true,
                gap: 10,
                padding: Insets::new_same(10),
            })));
        // ── Settings dialog (hidden, drawn last so it appears on top) ────────────
        scene.add_view_to_parent(make_label("dlg_title", "Settings"), &DIALOG_ID);
        scene.add_view_to_parent(make_label("dlg_font_lbl", "Font Size"), &DIALOG_ID);
        scene.add_view_to_parent(
            make_toggle_group(&FONT_SIZE_ID, vec!["Small", "Medium", "Large"], 1),
            &DIALOG_ID,
        );
        scene.add_view_to_parent(make_label("dlg_bl_lbl", "Backlight"), &DIALOG_ID);
        scene.add_view_to_parent(
            make_toggle_group(&BACKLIGHT_ID, vec!["Off", "Low", "High"], 2),
            &DIALOG_ID,
        );
        scene.add_view_to_parent(make_label("dlg_orient_lbl", "Orientation"), &DIALOG_ID);
        scene.add_view_to_parent(
            make_toggle_group(
                &ORIENTATION_ID,
                vec!["Port", "Land", "R.Port", "R.Land"],
                0,
            ),
            &DIALOG_ID,
        );
        scene.add_view_to_parent(make_button(&ViewId::new("sync_time"), "Sync Time"), &DIALOG_ID);
        scene.add_view_to_parent(make_label("dlg_battery", "Battery: 85%  (Charging)"), &DIALOG_ID);
        scene.add_view_to_parent(make_button(&ViewId::new("dialog_close"), "Close"), &DIALOG_ID);
        scene.add_view_to_root(dialog_panel);
    }

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

static FONT_BYTES: &[u8] = include_bytes!("../fonts/AtkinsonHyperlegible-Regular.ttf");
static BODY_FONT_BYTES: &[u8] = include_bytes!("../fonts/NoticiaText-Regular.ttf");
static BOLD_FONT_BYTES: &[u8] = include_bytes!("../fonts/AtkinsonHyperlegible-Bold.ttf");

/// Parse both theme fonts and leak them into `'static` memory.
/// Works on both std (simulator) and no_std+alloc (ESP) since both provide `Box`.
fn load_fonts() -> (&'static Font, &'static Font, &'static Font) {
    let font = Box::leak(Box::new(
        fontdue::Font::from_bytes(FONT_BYTES, fontdue::FontSettings::default())
            .expect("AtkinsonHyperlegible-Regular parse failed"),
    ));
    let bold = Box::leak(Box::new(
        fontdue::Font::from_bytes(BOLD_FONT_BYTES, fontdue::FontSettings::default())
            .expect("NoticiaText-Bold parse failed"),
    ));
    let body = Box::leak(Box::new(
        fontdue::Font::from_bytes(BODY_FONT_BYTES, fontdue::FontSettings::default())
            .expect("NoticiaText-Regular parse failed"),
    ));
    (font, bold, body)
}

fn make_theme(font: &'static Font, bold_font: &'static Font) -> Theme {
    Theme {
        standard: ViewStyle { fill: Rgb565::WHITE, text: Rgb565::BLACK },
        accented: ViewStyle { fill: Rgb565::WHITE, text: Rgb565::BLACK },
        selected: ViewStyle { fill: Rgb565::BLACK, text: Rgb565::WHITE },
        panel:    ViewStyle { fill: Rgb565::WHITE, text: Rgb565::BLACK },
        font:      FontKind::TrueType { size: 13.0, font },
        bold_font: FontKind::TrueType { size: 13.0, font: bold_font },
    }
}

fn set_font_size(theme: &mut Theme, font: &'static Font, bold_font: &'static Font, size: f32) {
    theme.font = FontKind::TrueType {
        font,
        size,
    };
    theme.bold_font = FontKind::TrueType {
        font:bold_font,
        size
    }
}

#[cfg(feature = "simulator")]
fn main() {
    use embedded_graphics::geometry::Size;
    use embedded_graphics_simulator::{
        sdl2::Keycode, OutputSettingsBuilder, SimulatorDisplay, SimulatorEvent, Window,
    };

    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let mut hw = SimHardware::new();
    let (mut win_w, mut win_h) = hw.orientation().logical_size();

    let mut display: SimulatorDisplay<Rgb565> =
        SimulatorDisplay::new(Size::new(win_w as u32, win_h as u32));
    let settings = OutputSettingsBuilder::new().scale(1).build();
    let mut window = Window::new("ereader_ui", &settings);

    let (font, bold_font, body_font) = load_fonts();
    let mut scene = make_scene(body_font, win_w, win_h);
    let mut theme = make_theme(font, bold_font);
    set_font_size(&mut theme, font, bold_font, 13.0);
    let handlers: Vec<Callback> = vec![handle_click];

    let epub = EpubArchive::new(EPUB_DATA).expect("sherlock_holmes.epub parse failed");
    let mut cfg = layout_cfg(body_font, hw.font_size(), win_w, win_h);
    let mut session = BookSession::new(&epub, &cfg).expect("BookSession init failed");
    update_content(&mut scene, &session, font_px_for(hw.font_size()));

    'running: loop {
        let dirty = scene.dirty_rect.clone();
        {
            let mut ctx = EmbeddedDrawingContext::new(&mut display);
            ctx.clip = dirty.clone();
            layout_scene(&mut scene, &theme);
            draw_scene(&mut scene, &mut ctx, &theme);
        }

        scene.mark_layout_dirty();
        window.update(&display);

        let events: Vec<_> = window.events().collect();
        for event in events {
            match event {
                SimulatorEvent::Quit => break 'running,
                // Keyboard shortcuts: arrow keys / Space simulate physical buttons.
                SimulatorEvent::KeyDown { keycode: Keycode::Left, repeat: false, .. }
                | SimulatorEvent::KeyDown { keycode: Keycode::Backspace, repeat: false, .. } => {
                    nav_prev_page(&mut hw, &mut scene, &epub, &mut cfg, &mut session);
                }
                SimulatorEvent::KeyDown { keycode: Keycode::Right, repeat: false, .. }
                | SimulatorEvent::KeyDown { keycode: Keycode::Space, repeat: false, .. } => {
                    nav_next_page(&mut hw, &mut scene, &epub, &mut cfg, &mut session);
                }
                SimulatorEvent::MouseButtonUp { point, .. } => {
                    if let Some(input) =
                        click_at(&mut scene, &handlers, GPoint::new(point.x, point.y))
                    {
                        if let Some(OutputAction::Command(ref cmd)) = input.action {
                            if input.source == ORIENTATION_ID {
                                hw.set_orientation(Orientation::from_cmd(cmd.as_str()));
                                let (new_w, new_h) = hw.orientation().logical_size();
                                if new_w != win_w || new_h != win_h {
                                    win_w = new_w;
                                    win_h = new_h;
                                    scene.resize(Bounds::new(0, 0, win_w, win_h));
                                    display = SimulatorDisplay::new(
                                        Size::new(win_w as u32, win_h as u32),
                                    );
                                    window = Window::new("ereader_ui", &settings);
                                    cfg = layout_cfg(body_font, hw.font_size(), win_w, win_h);
                                    session.reader.relayout(&cfg);
                                    update_content(&mut scene, &session, font_px_for(hw.font_size()));
                                }
                            } else if input.source == FONT_SIZE_ID {
                                hw.set_font_size(FontSize::from_cmd(cmd.as_str()));
                                let font_size = match hw.font_size() {
                                    FontSize::Small => 13.0,
                                    FontSize::Medium => 16.0,
                                    FontSize::Large => 24.0,
                                };
                                set_font_size(&mut theme, font, bold_font, font_size);
                                cfg = layout_cfg(body_font, hw.font_size(), win_w, win_h);
                                session.reader.relayout(&cfg);
                                scene.mark_layout_dirty();
                                update_content(&mut scene, &session, font_px_for(hw.font_size()));
                            } else if input.source == BACKLIGHT_ID {
                                hw.set_backlight_level(BacklightLevel::from_cmd(cmd.as_str()));
                            }
                        }
                        if input.source == ViewId::new("sync_time") {
                            let t = hw.current_time_secs();
                            if let Some(view) = scene.get_view_mut(&ViewId::new("time")) {
                                view.title = format_time_utc(t);
                            }
                            scene.mark_layout_dirty();
                        } else if input.source == ViewId::new("prev_page") {
                            nav_prev_page(&mut hw, &mut scene, &epub, &mut cfg, &mut session);
                        } else if input.source == ViewId::new("next_page") {
                            nav_next_page(&mut hw, &mut scene, &epub, &mut cfg, &mut session);
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

fn nav_next_page(mut hw: &mut dyn HardwareAccess,
                 mut scene: &mut Scene,
                 epub: &EpubArchive,
                 cfg: &mut LayoutConfig,
                 session: &mut BookSession) {
    if session.reader.current_page + 1 >= session.reader.page_count() {
        session.next_chapter(&epub, &cfg).ok();
    } else {
        session.reader.turn_page(true);
    }
    update_content(&mut scene, &session, font_px_for(hw.font_size()));
}

fn nav_prev_page(mut hw: &mut dyn HardwareAccess,
                 mut scene: &mut Scene,
                 epub: &EpubArchive,
                 cfg: &mut LayoutConfig,
                 session: &mut BookSession) {
    if session.reader.current_page == 0 {
        session.prev_chapter(&epub, &cfg).ok();
    } else {
        session.reader.turn_page(false);
    }
    update_content(&mut scene, &session, font_px_for(hw.font_size()));
}

// ── ESP path ──────────────────────────────────────────────────────────────────
#[cfg(feature = "esp")]
use esp_backtrace as _;

#[cfg(feature = "esp")]
esp_bootloader_esp_idf::esp_app_desc!();

#[cfg(feature = "esp")]
use ereader::driver::display::{Display, DrawMode};
#[cfg(feature = "esp")]
use ereader::driver::gt911::GT911_ADDR_PRIMARY;
#[cfg(feature = "esp")]
use ereader::driver::Gt911;

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

// Deep sleep after 60 seconds of inactivity.
#[cfg(feature = "esp")]
const SLEEP_AFTER_SECS: u64 = 60;

// Keys 10–14 to avoid collisions with ereader_full (which uses 0–4).
#[cfg(feature = "esp")]
const NVS_RANGE: core::ops::Range<u32> = 0x9000..0xF000;
#[cfg(feature = "esp")]
const KEY_FONT:    u8 = 10;
#[cfg(feature = "esp")]
const KEY_BL:      u8 = 11;
#[cfg(feature = "esp")]
const KEY_ORI:     u8 = 12;
#[cfg(feature = "esp")]
const KEY_CHAPTER: u8 = 13;
#[cfg(feature = "esp")]
const KEY_ANCHOR:  u8 = 14;

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

/// Returns (chapter_idx, anchor_byte). Defaults: chapter 0, byte 0.
#[cfg(feature = "esp")]
fn load_position() -> (usize, usize) {
    let mut flash = FlashAdapter(FlashStorage::new());
    let mut cache = NoCache::new();
    let mut buf = [0u8; 64];
    let mut load = |key: u8| -> u32 {
        match block_on(map::fetch_item::<u8, u32, _>(
            &mut flash, NVS_RANGE, &mut cache, &mut buf, &key,
        )) {
            Ok(Some(v)) => v,
            _ => 0,
        }
    };
    let chapter = load(KEY_CHAPTER) as usize;
    let anchor  = load(KEY_ANCHOR)  as usize;
    log::info!("position loaded: chapter={} anchor={}", chapter, anchor);
    (chapter, anchor)
}

#[cfg(feature = "esp")]
fn save_position(chapter_idx: usize, anchor_byte: usize) {
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
    save(KEY_CHAPTER, chapter_idx as u32);
    save(KEY_ANCHOR,  anchor_byte as u32);
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
use embassy_executor::Spawner;
#[cfg(feature = "esp")]
use embassy_net::{
    udp::{PacketMetadata, UdpSocket}, IpAddress, IpEndpoint, Ipv4Address, Runner, StackResources};
#[cfg(feature = "esp")]
use embassy_time::{with_timeout, Duration, Instant, Timer as EmbassyTimer};
#[cfg(feature = "esp")]
use ereader::hardware::rtc_store_read;
#[cfg(feature = "esp")]
use esp_hal::{
    gpio::{Input, InputConfig, Pull},
    interrupt::software::SoftwareInterruptControl,
    ledc::{
        channel::{self, ChannelIFace},
        timer::{self, TimerIFace},
        LSGlobalClkSource, Ledc, LowSpeed,
    },
    rtc_cntl::{reset_reason, Rtc, SocResetReason},
    system::Cpu,
    time::Rate,
    timer::timg::TimerGroup,
};
#[cfg(feature = "esp")]
use esp_radio::wifi::{sta::StationConfig, Config, ControllerConfig, Interface};
use Flex::Shrink;
use fontdue::Font;
use iris_ui::input::OutputAction;
use iris_ui::panel::{make_panel, PanelState};
use iris_ui::view::Flex::Grow;
use log::info;
#[cfg(feature = "esp")]
use static_cell::StaticCell;
use ereader::bookview::{draw_book_content, layout_cfg, update_content, BookState, CONTENT_ID};

// WiFi credentials — set WIFI_SSID and WIFI_PASS at build time.
#[cfg(feature = "esp")]
const SSID:     &str = match option_env!("WIFI_SSID") { Some(s) => s, None => "SSID" };
#[cfg(feature = "esp")]
const PASSWORD: &str = match option_env!("WIFI_PASS") { Some(s) => s, None => "PASSWORD" };
// Set to false to skip WiFi init and NTP sync entirely (e.g. when no network is available).
#[cfg(feature = "esp")]
const ENABLE_WIFI_NTP: bool = false;

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
async fn net_task(mut runner: Runner<'static, Interface<'static>>) {
    runner.run().await
}

/// Query time.google.com via NTP and return Unix seconds, or None on error.
#[cfg(feature = "esp")]
async fn query_ntp(stack: embassy_net::Stack<'static>) -> Option<u64> {
    let mut rx_meta = [PacketMetadata::EMPTY; 4];
    let mut rx_buf  = [0u8; 512];
    let mut tx_meta = [PacketMetadata::EMPTY; 4];
    let mut tx_buf  = [0u8; 256];
    let mut socket  = UdpSocket::new(stack, &mut rx_meta, &mut rx_buf, &mut tx_meta, &mut tx_buf);
    socket.bind(12345).ok()?;

    let endpoint = IpEndpoint::new(IpAddress::Ipv4(Ipv4Address::from_octets(NTP_ADDR)), 123);
    let mut pkt = [0u8; 48];
    pkt[0] = 0x1B; // LI=0, VN=3, Mode=3 (client)
    socket.send_to(&pkt, endpoint).await.ok()?;

    let (n, _) = socket.recv_from(&mut pkt).await.ok()?;
    if n < 48 { return None; }

    let ntp_secs = u32::from_be_bytes([pkt[40], pkt[41], pkt[42], pkt[43]]) as u64;
    if ntp_secs <= NTP_UNIX_OFFSET { return None; }
    Some(ntp_secs - NTP_UNIX_OFFSET)
}

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
    // Detect whether we woke from deep sleep or did a cold boot.
    let is_sleep_wakeup = reset_reason(Cpu::ProCpu) == Some(SocResetReason::CoreDeepSleep);

    // On wakeup restore from RTC fast memory (fast, no flash wear); on first
    // boot read persisted settings from NVS flash.
    let (font_idx, bl_idx, ori_idx, saved_chapter, saved_anchor) = if is_sleep_wakeup {
        let anchor  = rtc_store_read(0) as usize;
        let packed  = rtc_store_read(5);
        let font    = (packed & 0xF) as usize;
        let bl      = ((packed >> 4) & 0xF) as usize;
        let ori     = ((packed >> 8) & 0xF) as usize;
        let chapter = rtc_store_read(6) as usize;
        log::info!("woke from deep sleep: ch={} anchor={} font={} bl={} ori={}", chapter, anchor, font, bl, ori);
        (font, bl, ori, chapter, anchor)
    } else {
        let (font, bl, ori) = load_settings();
        let (chapter, anchor) = load_position();
        (font, bl, ori, chapter, anchor)
    };

    // Physical buttons: BOOT (GPIO0, active-low) = prev page; GPIO38 = next page.
    let btn_prev = Input::new(peripherals.GPIO0,  InputConfig::default().with_pull(Pull::Up));
    let btn_next = Input::new(peripherals.GPIO38, InputConfig::default().with_pull(Pull::Up));

    // Capture seed before rtc is moved into hw.
    let seed = rtc.current_time_us();
    let mut hw = EspHardware::new(
        bl_ch,
        rtc,
        btn_prev,
        btn_next,
        FontSize::from_index(font_idx),
        BacklightLevel::from_index(bl_idx),
        Orientation::from_index(ori_idx),
    );
    let (lw, lh) = hw.orientation().logical_size();
    let mut bridge = Rgb565ToGray4::new(display, hw.orientation());
    let (font, bold_font, body_font) = load_fonts();
    let mut scene = make_scene(body_font, lw, lh);
    sync_settings_ui(&mut scene, font_idx, bl_idx, ori_idx);
    let mut theme = make_theme(font, bold_font);
    let chrome_size = match hw.font_size() {
        FontSize::Small => 13.0, FontSize::Medium => 16.0, FontSize::Large => 24.0,
    };
    set_font_size(&mut theme, font, bold_font, chrome_size);
    let handlers = vec![handle_click as Callback];
    let mut was_touching = false;

    let epub = EpubArchive::new(EPUB_DATA).expect("epub parse");
    let mut cfg = layout_cfg(body_font, hw.font_size(), lw, lh);
    let mut session = if saved_chapter > 0 || saved_anchor > 0 {
        BookSession::restore(&epub, &cfg, saved_chapter, saved_anchor)
            .unwrap_or_else(|_| BookSession::new(&epub, &cfg).expect("epub load"))
    } else {
        BookSession::new(&epub, &cfg).expect("epub load")
    };
    update_content(&mut scene, &session, font_px_for(hw.font_size()));

    let mut last_interaction = Instant::now();

    // ── WiFi + NTP time sync ──────────────────────────────────────────────────
    // Only sync on cold boot. On deep-sleep wakeup the RTC already holds the
    // correct time so there is no need to reconnect WiFi.
    if ENABLE_WIFI_NTP {
        let station_config = Config::Station(
            StationConfig::default()
                .with_ssid(SSID)
                .with_password(PASSWORD.into()),
        );
        let (mut controller, interfaces) = esp_radio::wifi::new(
            peripherals.WIFI,
            ControllerConfig::default().with_initial_config(station_config),
        ).expect("wifi init");

        if !is_sleep_wakeup {
            let (stack, runner) = embassy_net::new(
                interfaces.station,
                embassy_net::Config::dhcpv4(Default::default()),
                mk_static!(StackResources<3>, StackResources::<3>::new()),
                seed,
            );
            spawner.spawn(net_task(runner).expect("net_task"));

            log::info!("NTP: connecting to '{}' ...", SSID);
            let ntp_result = with_timeout(Duration::from_secs(20), async {
                if let Err(e) = controller.connect_async().await {
                    log::warn!("NTP: wifi connect failed: {:?}", e);
                    return None;
                }
                log::info!("NTP: wifi connected, waiting for DHCP...");
                stack.wait_config_up().await;
                log::info!("NTP: DHCP obtained, querying time.google.com...");
                query_ntp(stack).await
            }).await;

            match ntp_result {
                Ok(Some(unix_secs)) => {
                    hw.set_current_time_secs(unix_secs);
                    let time_str = format_time_utc(unix_secs);
                    if let Some(view) = scene.get_view_mut(&ViewId::new("time")) {
                        view.title = time_str.clone();
                    }
                    scene.mark_layout_dirty();
                    log::info!("NTP synced: {}", time_str);
                }
                Ok(None) => log::warn!("NTP: query failed (no response or bad packet)"),
                Err(_)   => log::warn!("NTP: timed out after 20 s (SSID: '{}')", SSID),
            }

            controller.disconnect_async().await.ok();
            log::info!("NTP: wifi disconnected");
        } else {
            log::info!("NTP: skipped (woke from sleep, RTC time retained)");
        }
    } else {
        log::info!("WiFi/NTP disabled (ENABLE_WIFI_NTP = false)");
    }
    let _ = seed;

    loop {
        // Physical button handling: BOOT (GPIO0) = prev, GPIO38 = next.
        // Debounce by waiting for release before acting.
        if hw.button_prev_pressed() {
            while hw.button_prev_pressed() {
                EmbassyTimer::after(Duration::from_millis(10)).await;
            }
            nav_prev_page(&mut hw, &mut scene, &epub, &mut cfg, &mut session);
            save_position(session.chapter_idx, session.reader.anchor_byte);
            last_interaction = Instant::now();
        } else if hw.button_next_pressed() {
            while hw.button_next_pressed() {
                EmbassyTimer::after(Duration::from_millis(10)).await;
            }
            nav_next_page(&mut hw, &mut scene, &epub, &mut cfg, &mut session);
            save_position(session.chapter_idx, session.reader.anchor_byte);
            last_interaction = Instant::now();
        }

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
                ctx.clip = dirty_rect.clone();
                layout_scene(&mut scene, &theme);
                draw_scene(&mut scene, &mut ctx, &theme);
            }
            bridge.flush();
        }

        if let Some((tx, ty)) = bridge.display.read_touch(&mut gt911) {
            if !was_touching {
                let (lx, ly) = hw.orientation().phys_to_logical(tx, ty);
                if let Some(input) =
                    click_at(&mut scene, &handlers, GPoint::new(lx, ly))
                {
                    if let Some(OutputAction::Command(ref cmd)) = input.action {
                        if input.source == FONT_SIZE_ID {
                            hw.set_font_size(FontSize::from_cmd(cmd.as_str()));
                            let font_size = match hw.font_size() {
                                FontSize::Small => 13.0,
                                FontSize::Medium => 16.0,
                                FontSize::Large => 24.0,
                            };
                            set_font_size(&mut theme, font, bold_font, font_size);
                            let (cur_w, cur_h) = hw.orientation().logical_size();
                            cfg = layout_cfg(body_font, hw.font_size(), cur_w, cur_h);
                            session.reader.relayout(&cfg);
                            scene.mark_layout_dirty();
                            update_content(&mut scene, &session, font_px_for(hw.font_size()));
                            save_settings(hw.font_size().to_index(), hw.backlight_level().to_index(), hw.orientation().to_index());
                        } else if input.source == BACKLIGHT_ID {
                            hw.set_backlight_level(BacklightLevel::from_cmd(cmd.as_str()));
                            scene.mark_dirty_all();
                            save_settings(hw.font_size().to_index(), hw.backlight_level().to_index(), hw.orientation().to_index());
                        } else if input.source == ORIENTATION_ID {
                            hw.set_orientation(Orientation::from_cmd(cmd.as_str()));
                            bridge.orientation = hw.orientation();
                            let (new_w, new_h) = hw.orientation().logical_size();
                            // scene.bounds = Bounds::new(0, 0, new_w, new_h);
                            cfg = layout_cfg(body_font, hw.font_size(), new_w, new_h);
                            session.reader.relayout(&cfg);
                            scene.mark_layout_dirty();
                            update_content(&mut scene, &session, font_px_for(hw.font_size()));
                            save_settings(hw.font_size().to_index(), hw.backlight_level().to_index(), hw.orientation().to_index());
                        }
                    }
                    if input.source == ViewId::new("sync_time") {
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
                    } else if input.source == ViewId::new("prev_page") {
                        nav_prev_page(&mut hw, &mut scene, &epub, &mut cfg, &mut session);
                        save_position(session.chapter_idx, session.reader.anchor_byte);
                        last_interaction = Instant::now();
                    } else if input.source == ViewId::new("next_page") {
                        nav_next_page(&mut hw, &mut scene, &epub, &mut cfg, &mut session);
                        save_position(session.chapter_idx, session.reader.anchor_byte);
                        last_interaction = Instant::now();
                    } else {
                        last_interaction = Instant::now();
                    }
                }
            }
            was_touching = true;
        } else {
            was_touching = false;
        }

        // Enter deep sleep after inactivity timeout.
        if last_interaction.elapsed().as_secs() >= SLEEP_AFTER_SECS {
            log::info!("inactivity timeout — entering deep sleep");
            if let Some(v) = scene.get_view_mut(&ViewId::new("page")) {
                v.title = "Sleeping\u{2026} Press BOOT to wake".into();
            }
            scene.mark_dirty_all();
            // Render the sleep message before powering off.
            let sleep_dirty = scene.dirty_rect.clone();
            {
                let mut ctx = EmbeddedDrawingContext::new(&mut bridge);
                ctx.clip = sleep_dirty;
                layout_scene(&mut scene, &theme);
                draw_scene(&mut scene, &mut ctx, &theme);
            }
            bridge.flush();
            bridge.display.power_off();
            // On ESP: saves RTC state and enters deep sleep (never returns).
            // On simulator enter_deep_sleep is a no-op; reset the timer so we
            // don't loop immediately back into the sleep check.
            hw.enter_deep_sleep(session.chapter_idx, session.reader.anchor_byte);
            last_interaction = Instant::now();
            update_content(&mut scene, &session, font_px_for(hw.font_size()));
        }

        EmbassyTimer::after(Duration::from_millis(50)).await;
    }
}
