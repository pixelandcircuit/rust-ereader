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
use alloc::{boxed::Box, string::String, vec::Vec};
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::RgbColor;
use ereader::book::{Book, HtmlBook, TxtBook};
use ereader::epub::EpubArchive;
use ereader::font::font_px_for;
#[cfg(feature = "simulator")]
use ereader::hardware::SimHardware;
#[cfg(feature = "esp")]
use ereader::hardware::{load_cold_boot_position, load_settings, EspHardware};
use ereader::hardware::{BacklightLevel, FontSize, HardwareAccess, Orientation};
use ereader::layout::LayoutConfig;
use ereader::reader::BookSession;
use iris_ui::button::{make_button, make_full_button};
use iris_ui::device::EmbeddedDrawingContext;
use iris_ui::geom::{Bounds, Insets, Point as GPoint};
use iris_ui::label::make_label;
use iris_ui::layouts::{layout_hbox, layout_vbox};
use iris_ui::list_view::{make_list_view, ListState};
use iris_ui::scene::{click_at, draw_scene, layout_scene, Scene};
use iris_ui::toggle_group::{make_toggle_group, SelectOneOfState};
use iris_ui::view::{Align, Flex, View, ViewId};
use iris_ui::{Callback, FontKind, GuiEvent, LayoutEvent, Theme, ViewStyle};
use Align::Center;

const EPUB_DATA: &[u8] = include_bytes!("sherlock_holmes.epub");

const DIALOG_ID: ViewId = ViewId::new("dialog");
const LIBRARY_DIALOG_ID: ViewId = ViewId::new("library_dialog");
const ERROR_DIALOG_ID: ViewId = ViewId::new("error_dialog");
const FAST_SCROLL_PANEL_ID: ViewId = ViewId::new("fast_scroll_panel");
const FAST_SCROLL_W: i32 = 300;
const FAST_SCROLL_H: i32 = 80;
const ORIENTATION_ID: ViewId = ViewId::new("orientation");
const BACKLIGHT_ID: ViewId = ViewId::new("backlight");
const FONT_SIZE_ID: ViewId = ViewId::new("font_size");

const UI_FONT_SIZE_SMALL: f32 = 16.0;
const UI_FONT_SIZE_MEDIUM: f32 = 20.0;
const UI_FONT_SIZE_LARGE: f32 = 24.0;

fn handle_click(event: &mut GuiEvent) {
    if event.target == &ViewId::new("settings") {
        event.scene.show_view(&DIALOG_ID);
        event.scene.mark_dirty_all();
    } else if event.target == &ViewId::new("dialog_close") {
        event.scene.hide_view(&DIALOG_ID);
        event.scene.mark_dirty_all();
    } else if event.target == &ViewId::new("library") {
        event.scene.show_view(&LIBRARY_DIALOG_ID);
        event.scene.mark_layout_dirty();
        event.scene.mark_dirty_all();
    } else if event.target == &ViewId::new("library_close") {
        event.scene.hide_view(&LIBRARY_DIALOG_ID);
        event.scene.mark_dirty_all();
    } else if event.target == &ViewId::new("error_dismiss") {
        event.scene.hide_view(&ERROR_DIALOG_ID);
        event.scene.mark_dirty_all();
    }
}

/// Sync the settings dialog toggle groups to reflect the actual loaded settings.
/// make_scene() hardcodes default selections; call this after loading persisted values.
fn sync_settings_ui(scene: &mut Scene, font_idx: usize, bl_idx: usize, ori_idx: usize) {
    for (id, idx) in [
        (FONT_SIZE_ID.clone(), font_idx),
        (BACKLIGHT_ID, bl_idx),
        (ORIENTATION_ID, ori_idx),
    ] {
        if let Some(v) = scene.get_view_mut(&id) {
            if let Some(s) = v.get_state::<SelectOneOfState>() {
                s.selected = idx;
            }
        }
    }
}

fn make_truncating_label(name: &ViewId, title: &str) -> View {
    View {
        name: name.clone(),
        title: title.into(),
        h_flex: Grow,
        v_flex: Shrink,
        layout: Some(|e| {
            let space_w = e.space.w;
            if let Some(view) = e.scene.get_view_mut(e.target) {
                let font = e.theme.font;
                let ch = font.char_height();
                view.bounds.size.w = space_w;
                view.bounds.size.h = ch + (ch / 2) * 2;
            }
        }),
        draw: Some(|e| {
            let font = e.theme.font;
            let style = iris_ui::gfx::TextStyle::new(font, &e.theme.standard.text);
            let pad = font.char_width();
            let available = (e.view.bounds.size.w - pad * 2).max(0);
            if font.str_width(&e.view.title) <= available {
                e.ctx.fill_text(&e.view.bounds, &e.view.title, &style);
            } else {
                let ellipsis_w = font.str_width("...");
                let max_text_w = (available - ellipsis_w).max(0);
                let mut truncated = String::new();
                let mut used = 0i32;
                for c in e.view.title.chars() {
                    let mut buf = [0u8; 4];
                    let cw = font.str_width(c.encode_utf8(&mut buf));
                    if used + cw > max_text_w {
                        break;
                    }
                    truncated.push(c);
                    used += cw;
                }
                truncated.push_str("…");
                e.ctx.fill_text(&e.view.bounds, &truncated, &style);
            }
        }),
        ..Default::default()
    }
}

fn make_h_spacer(id: &ViewId) -> View {
    View {
        name: id.clone(),
        h_flex: Grow,
        h_align: Center,
        v_flex: Shrink,
        v_align: Center,
        bounds: Bounds::new(0, 0, 10, 10),
        visible: true,
        title: "spacer".into(),
        draw: Some(|e| {
            // e.ctx.stroke_rect(&e.view.bounds, &e.theme.panel.text);
        }),
        input: None,
        layout: Some(|e| {
            if let Some(view) = e.scene.get_view_mut(&e.target) {
                if view.h_flex == Grow {
                    view.bounds.size.w = e.space.w;
                }
                if view.v_flex == Grow {
                    view.bounds.size.h = e.space.h;
                }
            }
        }),
        state: None,
    }
}

fn show_error_dialog(scene: &mut Scene, filename: &str) {
    if let Some(v) = scene.get_view_mut(&ViewId::new("err_msg")) {
        v.title = String::from(filename);
    }
    scene.show_view(&ERROR_DIALOG_ID);
    scene.mark_layout_dirty();
    scene.mark_dirty_all();
}

fn layout_fast_scroll_panel(e: &mut LayoutEvent) {
    if let Some(v) = e.scene.get_view_mut(&e.target) {
        v.bounds = Bounds::new(
            (e.space.w - FAST_SCROLL_W) / 2,
            (e.space.h - FAST_SCROLL_H) / 2,
            FAST_SCROLL_W,
            FAST_SCROLL_H,
        );
    }
}

/// Run a layout pass and build a LayoutConfig from the real content view bounds.
/// Must be called whenever the scene size or UI font changes.
fn cfg_from_scene(
    scene: &mut Scene,
    theme: &Theme,
    body_font: &'static Font,
    font_size: FontSize,
) -> LayoutConfig {
    layout_scene(scene, theme);
    let bounds = scene
        .get_view_bounds(&CONTENT_ID)
        .expect("content view not in scene");
    layout_cfg(body_font, font_size, bounds.size.w, bounds.size.h)
}

fn update_fast_scroll_label(
    scene: &mut Scene,
    chapter: usize,
    chapter_count: usize,
    page: usize,
    page_count: usize,
) {
    if let Some(v) = scene.get_view_mut(&ViewId::new("fast_scroll_label")) {
        v.title = format!(
            "Ch {}/{} · Pg {}/{}",
            chapter + 1,
            chapter_count,
            page + 1,
            page_count
        );
    }
    scene.mark_dirty_view(&FAST_SCROLL_PANEL_ID);
}

fn make_scene(body_font: &'static Font, w: i32, h: i32) -> Scene {
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
        scene.add_view_to_parent(make_button(&ViewId::new("library"), "Library"), &topbar_id);
        scene.add_view_to_parent(make_h_spacer(&ViewId::new("spacer1")), &topbar_id);
        scene.add_view_to_parent(make_label(&ViewId::new("time"), "--:-- --"), &topbar_id);
        scene.add_view_to_parent(make_label(&ViewId::new("battery"), "85%"), &topbar_id);
        scene.add_view_to_parent(make_full_button(&ViewId::new("settings"), "Settings",
                                                  "settings", false),&topbar_id);
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
                font: body_font,
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
        scene.add_view_to_parent(
            make_button(&ViewId::new("prev_page"), "< Prev"),
            &bottombar_id,
        );
        scene.add_view_to_parent(
            make_truncating_label(&ViewId::new("booktitle"), "Sherlock Holmes"),
            &bottombar_id,
        );

        scene.add_view_to_parent(
            make_label(&ViewId::new("chapter"), "Loading..."),
            &bottombar_id,
        );
        scene.add_view_to_parent(make_label(&ViewId::new("page"), ""), &bottombar_id);
        scene.add_view_to_parent(
            make_button(&ViewId::new("next_page"), "Next >"),
            &bottombar_id,
        );
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
        scene.add_view_to_parent(
            make_label(&ViewId::new("dlg_title"), "Settings"),
            &DIALOG_ID,
        );
        scene.add_view_to_parent(
            make_label(&ViewId::new("dlg_font_lbl"), "Font Size"),
            &DIALOG_ID,
        );
        scene.add_view_to_parent(
            make_toggle_group(&FONT_SIZE_ID, vec!["Small", "Medium", "Large"], 1),
            &DIALOG_ID,
        );
        scene.add_view_to_parent(
            make_label(&ViewId::new("dlg_bl_lbl"), "Backlight"),
            &DIALOG_ID,
        );
        scene.add_view_to_parent(
            make_toggle_group(&BACKLIGHT_ID, vec!["Off", "Low", "High"], 2),
            &DIALOG_ID,
        );
        scene.add_view_to_parent(
            make_label(&ViewId::new("dlg_orient_lbl"), "Orientation"),
            &DIALOG_ID,
        );
        scene.add_view_to_parent(
            make_toggle_group(&ORIENTATION_ID, vec!["Port", "Land", "R.Port", "R.Land"], 0),
            &DIALOG_ID,
        );
        scene.add_view_to_parent(
            make_button(&ViewId::new("sync_time"), "Sync Time"),
            &DIALOG_ID,
        );
        scene.add_view_to_parent(
            make_label(&ViewId::new("dlg_battery"), "Battery: 85%  (Charging)"),
            &DIALOG_ID,
        );
        scene.add_view_to_parent(
            make_button(&ViewId::new("dialog_close"), "Close"),
            &DIALOG_ID,
        );
        scene.add_view_to_root(dialog_panel);
    }

    // ── Library dialog (hidden, shown when Library button pressed) ───────────
    {
        let lib_panel = make_panel(&LIBRARY_DIALOG_ID)
            .with_layout(Some(layout_vbox))
            .with_h_flex(Flex::Grow)
            .with_v_flex(Flex::Grow)
            .with_visible(false)
            .with_state(Some(Box::new(PanelState {
                border_visible: true,
                gap: 10,
                padding: Insets::new_same(10),
            })));
        scene.add_view_to_parent(
            make_label(&ViewId::new("lib_title"), "Library"),
            &LIBRARY_DIALOG_ID,
        );
        scene.add_view_to_parent(
            make_list_view(&ViewId::new("lib_list"), vec![], 0)
                .with_h_flex(Flex::Grow)
                .with_v_flex(Flex::Grow)
                .with_h_align(Align::Start),
            &LIBRARY_DIALOG_ID,
        );
        let lib_btn_row_id = ViewId::new("lib_btn_row");
        scene.add_view_to_parent(
            make_button(&ViewId::new("library_close"), "Cancel"),
            &lib_btn_row_id,
        );
        scene.add_view_to_parent(
            make_button(&ViewId::new("library_read"), "Read"),
            &lib_btn_row_id,
        );
        let lib_btn_row = make_panel(&lib_btn_row_id)
            .with_layout(Some(layout_hbox))
            .with_h_flex(Flex::Grow)
            .with_state(Some(Box::new(PanelState {
                border_visible: false,
                gap: 5,
                padding: Insets::new_same(0),
            })));
        scene.add_view_to_parent(lib_btn_row, &LIBRARY_DIALOG_ID);
        scene.add_view_to_root(lib_panel);
    }

    // ── Error dialog (hidden, shown when a book fails to load) ───────────────
    {
        let err_panel = make_panel(&ERROR_DIALOG_ID)
            .with_layout(Some(layout_vbox))
            .with_h_flex(Flex::Grow)
            .with_v_flex(Flex::Grow)
            .with_visible(false)
            .with_state(Some(Box::new(PanelState {
                border_visible: true,
                gap: 10,
                padding: Insets::new_same(10),
            })));
        scene.add_view_to_parent(
            make_label(&ViewId::new("err_title"), "Cannot open file"),
            &ERROR_DIALOG_ID,
        );
        scene.add_view_to_parent(make_label(&ViewId::new("err_msg"), ""), &ERROR_DIALOG_ID);
        scene.add_view_to_parent(
            make_button(&ViewId::new("error_dismiss"), "Dismiss"),
            &ERROR_DIALOG_ID,
        );
        scene.add_view_to_root(err_panel);
    }

    // ── Fast-scroll overlay (hidden; shows page indicator while button held) ──
    {
        let fs_panel = make_panel(&FAST_SCROLL_PANEL_ID)
            .with_layout(Some(layout_fast_scroll_panel))
            .with_visible(false)
            .with_state(Some(Box::new(PanelState {
                border_visible: true,
                gap: 0,
                padding: Insets::new_same(10),
            })));
        scene.add_view_to_parent(
            make_label(&ViewId::new("fast_scroll_label"), "Page 0 / 0"),
            &FAST_SCROLL_PANEL_ID,
        );
        scene.add_view_to_root(fs_panel);
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
        standard: ViewStyle {
            fill: Rgb565::WHITE,
            text: Rgb565::BLACK,
        },
        accented: ViewStyle {
            fill: Rgb565::WHITE,
            text: Rgb565::BLACK,
        },
        selected: ViewStyle {
            fill: Rgb565::BLACK,
            text: Rgb565::WHITE,
        },
        panel: ViewStyle {
            fill: Rgb565::WHITE,
            text: Rgb565::BLACK,
        },
        font: FontKind::TrueType {
            size: UI_FONT_SIZE_MEDIUM,
            font,
        },
        bold_font: FontKind::TrueType {
            size: UI_FONT_SIZE_MEDIUM,
            font: bold_font,
        },
    }
}

#[cfg(feature = "simulator")]
fn main() {
    use embedded_graphics::geometry::Size;
    use embedded_graphics_simulator::{
        sdl2::Keycode, OutputSettingsBuilder, SimulatorDisplay, SimulatorEvent, Window,
    };
    use std::time::Instant;

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
    let handlers: Vec<Callback> = vec![handle_click];

    let mut book: Box<dyn Book> =
        Box::new(EpubArchive::new(EPUB_DATA).expect("sherlock_holmes.epub parse failed"));
    let mut cfg = cfg_from_scene(&mut scene, &theme, body_font, hw.font_size());
    let mut session = BookSession::new(book.as_ref(), &cfg).expect("BookSession init failed");
    let mut current_filename = String::from("__embedded__");
    update_content(&mut scene, &session, font_px_for(hw.font_size()));

    // Fast-scroll hold state (Up/Down arrow keys).
    let mut fs_forward = false;
    let mut fs_pressed_at: Option<Instant> = None;
    let mut fs_active = false;
    let mut fs_target: usize = 0;
    let mut fs_last_step = Instant::now();

    scene.mark_layout_dirty();
    'running: loop {
        // Advance fast-scroll page counter while a direction key is held.
        if let Some(pressed_at) = fs_pressed_at {
            if !fs_active && pressed_at.elapsed().as_millis() >= 1000 {
                fs_active = true;
                fs_target = session.reader.current_page;
                fs_last_step = Instant::now();
                update_fast_scroll_label(
                    &mut scene,
                    session.chapter_idx,
                    session.chapter_count(),
                    fs_target,
                    session.reader.page_count(),
                );
                scene.show_view(&FAST_SCROLL_PANEL_ID);
                scene.mark_layout_dirty();
            }
            if fs_active && fs_last_step.elapsed().as_millis() >= 200 {
                if fs_forward {
                    if fs_target + 1 >= session.reader.page_count() {
                        if session.chapter_idx + 1 < session.chapter_count() {
                            session
                                .go_to_chapter(session.chapter_idx + 1, book.as_ref(), &cfg)
                                .ok();
                            fs_target = 0;
                        }
                    } else {
                        fs_target += 1;
                    }
                } else if fs_target == 0 {
                    if session.chapter_idx > 0 {
                        session
                            .go_to_chapter(session.chapter_idx - 1, book.as_ref(), &cfg)
                            .ok();
                        fs_target = session.reader.page_count().saturating_sub(1);
                    }
                } else {
                    fs_target -= 1;
                }
                fs_last_step = Instant::now();
                update_fast_scroll_label(
                    &mut scene,
                    session.chapter_idx,
                    session.chapter_count(),
                    fs_target,
                    session.reader.page_count(),
                );
            }
        }

        if (!scene.dirty_rect.is_empty()) {
            info!("clip rect {}", scene.dirty_rect);
            let dirty = scene.dirty_rect.clone();
            let mut ctx = EmbeddedDrawingContext::new(&mut display);
            ctx.clip = dirty.clone();
            layout_scene(&mut scene, &theme);
            draw_scene(&mut scene, &mut ctx, &theme);
            window.update(&display);
        }

        let events: Vec<_> = window.events().collect();
        for event in events {
            match event {
                SimulatorEvent::Quit => break 'running,
                // Keyboard shortcuts: arrow keys / Space simulate physical buttons.
                SimulatorEvent::KeyDown {
                    keycode: Keycode::Left,
                    repeat: false,
                    ..
                }
                | SimulatorEvent::KeyDown {
                    keycode: Keycode::Backspace,
                    repeat: false,
                    ..
                } => {
                    nav_prev_page(&mut hw, &mut scene, book.as_ref(), &mut cfg, &mut session);
                }
                SimulatorEvent::KeyDown {
                    keycode: Keycode::Right,
                    repeat: false,
                    ..
                }
                | SimulatorEvent::KeyDown {
                    keycode: Keycode::Space,
                    repeat: false,
                    ..
                } => {
                    nav_next_page(&mut hw, &mut scene, book.as_ref(), &mut cfg, &mut session);
                }
                // Fast-scroll: hold Up or Down for >1 s to scan pages without rendering content.
                SimulatorEvent::KeyDown {
                    keycode: Keycode::Down,
                    repeat: false,
                    ..
                } => {
                    if fs_pressed_at.is_none() {
                        fs_forward = true;
                        fs_pressed_at = Some(Instant::now());
                    }
                }
                SimulatorEvent::KeyDown {
                    keycode: Keycode::Up,
                    repeat: false,
                    ..
                } => {
                    if fs_pressed_at.is_none() {
                        fs_forward = false;
                        fs_pressed_at = Some(Instant::now());
                    }
                }
                SimulatorEvent::KeyUp {
                    keycode: Keycode::Up,
                    ..
                }
                | SimulatorEvent::KeyUp {
                    keycode: Keycode::Down,
                    ..
                } => {
                    if fs_active {
                        session.reader.go_to_page(fs_target);
                        update_content(&mut scene, &session, font_px_for(hw.font_size()));
                        scene.hide_view(&FAST_SCROLL_PANEL_ID);
                        scene.mark_dirty_all();
                    }
                    fs_pressed_at = None;
                    fs_active = false;
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
                                    display = SimulatorDisplay::new(Size::new(
                                        win_w as u32,
                                        win_h as u32,
                                    ));
                                    window = Window::new("ereader_ui", &settings);
                                    cfg = cfg_from_scene(&mut scene, &theme, body_font, hw.font_size());
                                    session.reader.relayout(&cfg);
                                    update_content(
                                        &mut scene,
                                        &session,
                                        font_px_for(hw.font_size()),
                                    );
                                }
                            } else if input.source == FONT_SIZE_ID {
                                hw.set_font_size(FontSize::from_cmd(cmd.as_str()));
                                cfg = cfg_from_scene(&mut scene, &theme, body_font, hw.font_size());
                                session.reader.relayout(&cfg);
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
                        } else if input.source == ViewId::new("prev_page") {
                            nav_prev_page(
                                &mut hw,
                                &mut scene,
                                book.as_ref(),
                                &mut cfg,
                                &mut session,
                            );
                        } else if input.source == ViewId::new("next_page") {
                            nav_next_page(
                                &mut hw,
                                &mut scene,
                                book.as_ref(),
                                &mut cfg,
                                &mut session,
                            );
                        } else if input.source == ViewId::new("library") {
                            let files = hw.list_book_files();
                            if let Some(v) = scene.get_view_mut(&ViewId::new("lib_list")) {
                                if let Some(s) = v.get_state::<ListState>() {
                                    s.items = files;
                                    s.selected = 0;
                                }
                            }
                            scene.mark_layout_dirty();
                            scene.mark_dirty_all();
                        } else if input.source == ViewId::new("lib_list") {
                            scene.mark_dirty_all();
                        } else if input.source == ViewId::new("library_read") {
                            let filename = scene
                                .get_view_mut(&ViewId::new("lib_list"))
                                .and_then(|v| v.get_state::<ListState>())
                                .and_then(|s| s.items.get(s.selected).cloned());
                            if let Some(filename) = filename {
                                if let Some(data) = hw.load_book_file(&filename) {
                                    hw.save_bookmark(
                                        &current_filename,
                                        session.chapter_idx,
                                        session.reader.anchor_byte,
                                    );
                                    let new_book = book_from_data(&filename, data);
                                    cfg = cfg_from_scene(&mut scene, &theme, body_font, hw.font_size());
                                    let new_session = match hw.load_bookmark(&filename) {
                                        Some((ch, anchor)) => BookSession::restore(
                                            new_book.as_ref(),
                                            &cfg,
                                            ch,
                                            anchor,
                                        )
                                        .or_else(|_| BookSession::new(new_book.as_ref(), &cfg)),
                                        None => BookSession::new(new_book.as_ref(), &cfg),
                                    };
                                    if let Ok(s) = new_session {
                                        current_filename = filename.clone();
                                        session = s;
                                        book = new_book;
                                        update_content(
                                            &mut scene,
                                            &session,
                                            font_px_for(hw.font_size()),
                                        );
                                        if let Some(v) =
                                            scene.get_view_mut(&ViewId::new("booktitle"))
                                        {
                                            v.title = filename.clone();
                                        }
                                    } else {
                                        scene.hide_view(&LIBRARY_DIALOG_ID);
                                        show_error_dialog(&mut scene, &filename);
                                    }
                                    scene.hide_view(&LIBRARY_DIALOG_ID);
                                    scene.mark_dirty_all();
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

fn book_from_data(filename: &str, data: Vec<u8>) -> Box<dyn Book> {
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

fn calc_font_size(font_size: FontSize) -> f32 {
    match font_size {
        FontSize::Small => UI_FONT_SIZE_SMALL,
        FontSize::Medium => UI_FONT_SIZE_MEDIUM,
        FontSize::Large => UI_FONT_SIZE_LARGE,
    }
}

fn nav_next_page(
    hw: &mut dyn HardwareAccess,
    scene: &mut Scene,
    epub: &dyn Book,
    cfg: &mut LayoutConfig,
    session: &mut BookSession,
) {
    if session.reader.current_page + 1 >= session.reader.page_count() {
        session.next_chapter(epub, cfg).ok();
    } else {
        session.reader.turn_page(true);
    }
    update_content(scene, &session, font_px_for(hw.font_size()));
}

fn nav_prev_page(
    hw: &mut dyn HardwareAccess,
    scene: &mut Scene,
    epub: &dyn Book,
    cfg: &mut LayoutConfig,
    session: &mut BookSession,
) {
    if session.reader.current_page == 0 {
        session.prev_chapter(epub, cfg).ok();
    } else {
        session.reader.turn_page(false);
    }
    update_content(scene, &session, font_px_for(hw.font_size()));
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

// Deep sleep after 60 seconds of inactivity.
#[cfg(feature = "esp")]
const SLEEP_AFTER_SECS: u64 = 60;

/// Wraps the Gray4 e-paper display and presents an Rgb565 DrawTarget for iris-ui.
/// Converts Rgb565 luminance to 4-bit gray and applies orientation rotation so
/// the logical coordinate space matches what the user sees.
#[cfg(feature = "esp")]
struct Rgb565ToGray4<'a> {
    display: Display<'a>,
    orientation: Orientation,
}

#[cfg(feature = "esp")]
impl<'a> Rgb565ToGray4<'a> {
    fn new(display: Display<'a>, orientation: Orientation) -> Self {
        Self {
            display,
            orientation,
        }
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
            let (px, py) = self
                .orientation
                .logical_to_phys(pix.0.x as u16, pix.0.y as u16);
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
    udp::{PacketMetadata, UdpSocket},
    IpAddress, IpEndpoint, Ipv4Address, Runner, StackResources,
};
#[cfg(feature = "esp")]
use embassy_time::{with_timeout, Duration, Instant, Timer as EmbassyTimer};
use ereader::bookview::{draw_book_content, layout_cfg, update_content, BookState, CONTENT_ID};
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
use fontdue::Font;
use iris_ui::input::OutputAction;
use iris_ui::panel::{make_panel, PanelState};
use iris_ui::view::Flex::Grow;
use log::info;
#[cfg(feature = "esp")]
use static_cell::StaticCell;
use Flex::Shrink;

// WiFi credentials — set WIFI_SSID and WIFI_PASS at build time.
#[cfg(feature = "esp")]
const SSID: &str = match option_env!("WIFI_SSID") {
    Some(s) => s,
    None => "SSID",
};
#[cfg(feature = "esp")]
const PASSWORD: &str = match option_env!("WIFI_PASS") {
    Some(s) => s,
    None => "PASSWORD",
};
// Set to false to skip WiFi init and NTP sync entirely (e.g. when no network is available).
#[cfg(feature = "esp")]
const ENABLE_WIFI_NTP: bool = false;

#[cfg(feature = "esp")]
const NTP_ADDR: [u8; 4] = [216, 239, 35, 0]; // time.google.com
#[cfg(feature = "esp")]
const NTP_UNIX_OFFSET: u64 = 2_208_988_800; // NTP epoch → Unix epoch

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
    let mut rx_buf = [0u8; 512];
    let mut tx_meta = [PacketMetadata::EMPTY; 4];
    let mut tx_buf = [0u8; 256];
    let mut socket = UdpSocket::new(stack, &mut rx_meta, &mut rx_buf, &mut tx_meta, &mut tx_buf);
    socket.bind(12345).ok()?;

    let endpoint = IpEndpoint::new(IpAddress::Ipv4(Ipv4Address::from_octets(NTP_ADDR)), 123);
    let mut pkt = [0u8; 48];
    pkt[0] = 0x1B; // LI=0, VN=3, Mode=3 (client)
    socket.send_to(&pkt, endpoint).await.ok()?;

    let (n, _) = socket.recv_from(&mut pkt).await.ok()?;
    if n < 48 {
        return None;
    }

    let ntp_secs = u32::from_be_bytes([pkt[40], pkt[41], pkt[42], pkt[43]]) as u64;
    if ntp_secs <= NTP_UNIX_OFFSET {
        return None;
    }
    Some(ntp_secs - NTP_UNIX_OFFSET)
}

#[cfg(feature = "esp")]
#[esp_rtos::main]
async fn main(spawner: Spawner) -> ! {
    use esp_println::println;

    esp_println::logger::init_logger(log::LevelFilter::Info);

    let config = esp_hal::Config::default().with_cpu_clock(esp_hal::clock::CpuClock::_240MHz);
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
    let timg0 = TimerGroup::new(peripherals.TIMG0);
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
    lstimer0
        .configure(timer::config::Config {
            duty: timer::config::Duty::Duty8Bit,
            clock_source: timer::LSClockSource::APBClk,
            frequency: Rate::from_khz(1),
        })
        .unwrap();
    let mut bl_ch = ledc.channel(channel::Number::Channel0, peripherals.GPIO11);
    bl_ch
        .configure(channel::config::Config {
            timer: &lstimer0,
            duty_pct: 100,
            drive_mode: esp_hal::gpio::DriveMode::PushPull,
        })
        .unwrap();
    // Detect whether we woke from deep sleep or did a cold boot.
    let is_sleep_wakeup = reset_reason(Cpu::ProCpu) == Some(SocResetReason::CoreDeepSleep);

    // On wakeup restore from RTC fast memory (fast, no flash wear); on first
    // boot read persisted settings from NVS flash.
    let (font_idx, bl_idx, ori_idx, saved_chapter, saved_anchor) = if is_sleep_wakeup {
        let anchor = rtc_store_read(0) as usize;
        let packed = rtc_store_read(5);
        let font = (packed & 0xF) as usize;
        let bl = ((packed >> 4) & 0xF) as usize;
        let ori = ((packed >> 8) & 0xF) as usize;
        let chapter = rtc_store_read(6) as usize;
        log::info!(
            "woke from deep sleep: ch={} anchor={} font={} bl={} ori={}",
            chapter,
            anchor,
            font,
            bl,
            ori
        );
        (font, bl, ori, chapter, anchor)
    } else {
        let (font, bl, ori) = load_settings();
        let (chapter, anchor) = load_cold_boot_position();
        (font, bl, ori, chapter, anchor)
    };

    // Physical buttons: BOOT (GPIO0, active-low) = prev page; GPIO38 = next page.
    let btn_prev = Input::new(
        peripherals.GPIO0,
        InputConfig::default().with_pull(Pull::Up),
    );
    let btn_next = Input::new(
        peripherals.GPIO38,
        InputConfig::default().with_pull(Pull::Up),
    );

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
    let handlers = vec![handle_click as Callback];
    let mut was_touching = false;

    let mut book: Box<dyn Book> = Box::new(EpubArchive::new(EPUB_DATA).expect("epub parse"));
    let mut cfg = cfg_from_scene(&mut scene, &theme, body_font, hw.font_size());
    let mut current_filename = String::from("__embedded__");
    let mut session = if saved_chapter > 0 || saved_anchor > 0 {
        BookSession::restore(book.as_ref(), &cfg, saved_chapter, saved_anchor)
            .unwrap_or_else(|_| BookSession::new(book.as_ref(), &cfg).expect("epub load"))
    } else {
        BookSession::new(book.as_ref(), &cfg).expect("epub load")
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
        )
        .expect("wifi init");

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
            })
            .await;

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
                Err(_) => log::warn!("NTP: timed out after 20 s (SSID: '{}')", SSID),
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

    'running: loop {
        // Physical button handling: BOOT (GPIO0) = prev, GPIO38 = next.
        // Short press → single page turn. Hold > 1 s → fast-scroll mode: a
        // small overlay shows the target page number, incrementing every 200 ms.
        // The book content only re-renders on release.
        let btn_pressed = if hw.button_prev_pressed() {
            Some(false)
        } else if hw.button_next_pressed() {
            Some(true)
        } else {
            None
        };
        if let Some(forward) = btn_pressed {
            let pressed_at = Instant::now();
            let mut fs_active = false;
            let mut fs_target = 0usize;
            let mut fs_last_step = Instant::now();

            loop {
                let still_held = if forward {
                    hw.button_next_pressed()
                } else {
                    hw.button_prev_pressed()
                };
                if !still_held {
                    break;
                }

                if !fs_active && pressed_at.elapsed().as_millis() >= 1000 {
                    fs_active = true;
                    fs_target = session.reader.current_page;
                    fs_last_step = Instant::now();
                    update_fast_scroll_label(
                        &mut scene,
                        session.chapter_idx,
                        session.chapter_count(),
                        fs_target,
                        session.reader.page_count(),
                    );
                    scene.show_view(&FAST_SCROLL_PANEL_ID);
                    scene.mark_layout_dirty();
                }

                if fs_active && fs_last_step.elapsed().as_millis() >= 200 {
                    if forward {
                        if fs_target + 1 >= session.reader.page_count() {
                            if session.chapter_idx + 1 < session.chapter_count() {
                                session
                                    .go_to_chapter(session.chapter_idx + 1, book.as_ref(), &cfg)
                                    .ok();
                                fs_target = 0;
                            }
                        } else {
                            fs_target += 1;
                        }
                    } else if fs_target == 0 {
                        if session.chapter_idx > 0 {
                            session
                                .go_to_chapter(session.chapter_idx - 1, book.as_ref(), &cfg)
                                .ok();
                            fs_target = session.reader.page_count().saturating_sub(1);
                        }
                    } else {
                        fs_target -= 1;
                    }
                    fs_last_step = Instant::now();
                    update_fast_scroll_label(
                        &mut scene,
                        session.chapter_idx,
                        session.chapter_count(),
                        fs_target,
                        session.reader.page_count(),
                    );
                }

                // Redraw only the panel while held (partial refresh).
                let dirty_rect = scene.dirty_rect.clone();
                if !dirty_rect.is_empty() {
                    let (sw, sh) = hw.orientation().logical_size();
                    let needs_full = dirty_rect.size.w >= sw && dirty_rect.size.h >= sh;
                    if needs_full {
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

                EmbassyTimer::after(Duration::from_millis(10)).await;
            }

            if fs_active {
                scene.hide_view(&FAST_SCROLL_PANEL_ID);
                session.reader.go_to_page(fs_target);
                update_content(&mut scene, &session, font_px_for(hw.font_size()));
                scene.mark_dirty_all();
            } else if forward {
                nav_next_page(&mut hw, &mut scene, book.as_ref(), &mut cfg, &mut session);
            } else {
                nav_prev_page(&mut hw, &mut scene, book.as_ref(), &mut cfg, &mut session);
            }
            hw.save_bookmark(&current_filename, session.chapter_idx, session.reader.anchor_byte);
            last_interaction = Instant::now();
        }

        let dirty_rect = scene.dirty_rect.clone();
        let was_dirty = !dirty_rect.is_empty();
        let (scene_w, scene_h) = hw.orientation().logical_size();
        let needs_full_refresh = dirty_rect.size.w >= scene_w && dirty_rect.size.h >= scene_h;

        if was_dirty {
            info!("clip rect {}", scene.dirty_rect);
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
                if let Some(input) = click_at(&mut scene, &handlers, GPoint::new(lx, ly)) {
                    if let Some(OutputAction::Command(ref cmd)) = input.action {
                        if input.source == FONT_SIZE_ID {
                            hw.set_font_size(FontSize::from_cmd(cmd.as_str()));
                            cfg = cfg_from_scene(&mut scene, &theme, body_font, hw.font_size());
                            session.reader.relayout(&cfg);
                            scene.mark_layout_dirty();
                            update_content(&mut scene, &session, font_px_for(hw.font_size()));
                            hw.save_settings();
                        } else if input.source == BACKLIGHT_ID {
                            hw.set_backlight_level(BacklightLevel::from_cmd(cmd.as_str()));
                            scene.mark_dirty_all();
                            hw.save_settings();
                        } else if input.source == ORIENTATION_ID {
                            hw.set_orientation(Orientation::from_cmd(cmd.as_str()));
                            bridge.orientation = hw.orientation();
                            let (new_w, new_h) = hw.orientation().logical_size();
                            scene.resize(Bounds::new(0, 0, new_w, new_h));
                            cfg = cfg_from_scene(&mut scene, &theme, body_font, hw.font_size());
                            session.reader.relayout(&cfg);
                            scene.mark_layout_dirty();
                            update_content(&mut scene, &session, font_px_for(hw.font_size()));
                            hw.save_settings();
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
                        nav_prev_page(&mut hw, &mut scene, book.as_ref(), &mut cfg, &mut session);
                        hw.save_bookmark(
                            &current_filename,
                            session.chapter_idx,
                            session.reader.anchor_byte,
                        );
                        last_interaction = Instant::now();
                    } else if input.source == ViewId::new("next_page") {
                        nav_next_page(&mut hw, &mut scene, book.as_ref(), &mut cfg, &mut session);
                        hw.save_bookmark(
                            &current_filename,
                            session.chapter_idx,
                            session.reader.anchor_byte,
                        );
                        last_interaction = Instant::now();
                    } else if input.source == ViewId::new("library") {
                        let files = hw.list_book_files();
                        if let Some(v) = scene.get_view_mut(&ViewId::new("lib_list")) {
                            if let Some(s) = v.get_state::<ListState>() {
                                s.items = files;
                                s.selected = 0;
                            }
                        }
                        scene.mark_layout_dirty();
                        scene.mark_dirty_all();
                        last_interaction = Instant::now();
                    } else if input.source == ViewId::new("lib_list") {
                        scene.mark_dirty_all();
                        last_interaction = Instant::now();
                    } else if input.source == ViewId::new("library_read") {
                        let filename = scene
                            .get_view_mut(&ViewId::new("lib_list"))
                            .and_then(|v| v.get_state::<ListState>())
                            .and_then(|s| s.items.get(s.selected).cloned());
                        if let Some(filename) = filename {
                            if let Some(data) = hw.load_book_file(&filename) {
                                hw.save_bookmark(
                                    &current_filename,
                                    session.chapter_idx,
                                    session.reader.anchor_byte,
                                );
                                let new_book = book_from_data(&filename, data);
                                cfg = cfg_from_scene(&mut scene, &theme, body_font, hw.font_size());
                                let new_session = match hw.load_bookmark(&filename) {
                                    Some((ch_idx, anchor)) => BookSession::restore(
                                        new_book.as_ref(),
                                        &cfg,
                                        ch_idx,
                                        anchor,
                                    )
                                    .or_else(|_| BookSession::new(new_book.as_ref(), &cfg)),
                                    None => BookSession::new(new_book.as_ref(), &cfg),
                                };
                                if let Ok(s) = new_session {
                                    current_filename = filename.clone();
                                    session = s;
                                    book = new_book;
                                    update_content(
                                        &mut scene,
                                        &session,
                                        font_px_for(hw.font_size()),
                                    );
                                    if let Some(v) = scene.get_view_mut(&ViewId::new("booktitle")) {
                                        v.title = filename.clone();
                                    }
                                } else {
                                    scene.hide_view(&LIBRARY_DIALOG_ID);
                                    show_error_dialog(&mut scene, &filename);
                                }
                                scene.hide_view(&LIBRARY_DIALOG_ID);
                                scene.mark_dirty_all();
                            }
                        }
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
            // Ensure the bookmark is current before powering off (flash survives sleep).
            hw.save_bookmark(
                &current_filename,
                session.chapter_idx,
                session.reader.anchor_byte,
            );
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

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(all(test, feature = "simulator"))]
mod tests {
    use super::*;
    use ereader::bookview::{layout_cfg, CONTENT_ID};
    use ereader::hardware::FontSize;
    use ereader::font::line_height;
    use iris_ui::scene::layout_scene;

    fn make_test_fonts() -> (&'static Font, &'static Font, &'static Font) {
        load_fonts()
    }

    /// The text layout engine (layout_chapter) breaks pages using cfg.screen_height.
    /// The renderer (render_ttf_text) draws lines while baseline < view.h - 12 px,
    /// where the first baseline is at view.h offset + 12 px — consuming 24 px total
    /// for top and bottom padding.  The two agree only when:
    ///
    ///   cfg.screen_height == content_view.h - 24
    ///
    /// This test builds the real scene, runs a layout pass to obtain the true content
    /// view bounds, then checks that invariant for every FontSize.
    #[test]
    fn layout_cfg_height_matches_content_view() {
        let (ui_font, bold_font, body_font) = make_test_fonts();
        let w = 960i32;
        let h = 540i32;

        let mut scene = make_scene(body_font, w, h);
        let mut theme = make_theme(ui_font, bold_font);
        layout_scene(&mut scene, &theme);

        let content_bounds = scene
            .get_view_bounds(&CONTENT_ID)
            .expect("content view not in scene");
        let content_w = content_bounds.size.w;
        let content_h = content_bounds.size.h as u32;

        for font_size in [FontSize::Small, FontSize::Medium, FontSize::Large] {
            let cfg = layout_cfg(body_font, font_size, content_w, content_h as i32);
            let render_pad_total = 24u32; // pad_y (12) top + pad_y (12) bottom
            let expected = content_h.saturating_sub(render_pad_total);
            assert_eq!(
                cfg.screen_height,
                expected,
                "FontSize::{:?}: layout uses {} px but render has {} px available ({} - {})",
                font_size,
                cfg.screen_height,
                expected,
                content_h,
                render_pad_total,
            );
        }
    }

    /// Each page's worth of lines as counted by the layout engine should all
    /// fit within the content view without clipping.
    #[test]
    fn page_line_count_fits_in_content_view() {
        let (ui_font, bold_font, body_font) = make_test_fonts();
        let w = 960i32;
        let h = 540i32;

        let mut scene = make_scene(body_font, w, h);
        let mut theme = make_theme(ui_font, bold_font);
        layout_scene(&mut scene, &theme);

        let content_bounds = scene
            .get_view_bounds(&CONTENT_ID)
            .expect("content view not in scene");
        let content_w = content_bounds.size.w;
        let content_h = content_bounds.size.h as u32;
        let render_usable = content_h.saturating_sub(24); // top+bottom pad_y

        for font_size in [FontSize::Small, FontSize::Medium, FontSize::Large] {
            let cfg = layout_cfg(body_font, font_size, content_w, content_h as i32);
            let font_px = ereader::font::font_px_for(font_size);
            let line_h = line_height(body_font, font_px) as u32 + 4; // matches render_ttf_text
            let layout_lines = cfg.screen_height / line_h;
            let render_lines = render_usable / line_h;
            assert!(
                layout_lines <= render_lines,
                "FontSize::{:?}: layout fits {} lines per page but render only shows {} \
                 (layout_h={}, render_usable={})",
                font_size,
                layout_lines,
                render_lines,
                cfg.screen_height,
                render_usable,
            );
        }
    }

}
