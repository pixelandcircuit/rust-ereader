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
use ereader::font::{font_px_for, AppFonts};
#[cfg(feature = "simulator")]
use ereader::hardware::SimHardware;

#[cfg(feature = "simulator")]
use std::time::Instant;

#[cfg(feature = "esp")]
use ereader::hardware::{
    load_cold_boot_position, load_last_filename, load_settings, save_last_filename, EspHardware,
};
use ereader::hardware::{BacklightLevel, FontSize, HardwareAccess, Orientation};
use ereader::layout::LayoutConfig;
use ereader::reader::BookSession;
use iris_ui::button::{make_button, make_full_button};
use iris_ui::device::EmbeddedDrawingContext;
use iris_ui::geom::{Bounds, Insets, Point as GPoint};
use iris_ui::label::make_label;
use iris_ui::layouts::{layout_centered_dialog, layout_hbox, layout_vbox};
use iris_ui::list_view::{make_list_view, ListState};
use iris_ui::scene::{click_at, draw_scene, layout_scene, Scene};
use iris_ui::toggle_group::{make_toggle_group, SelectOneOfState};
use iris_ui::view::{Align, Flex, View, ViewId};
use iris_ui::{Callback, FontKind, GuiEvent, LayoutEvent, Theme, ViewStyle};
use Align::Center;

const WELCOME_HTML: &[u8] = include_bytes!("welcome.html");

const SETTINGS_DIALOG_ID: ViewId = ViewId::new("settings_dialog");
const LIBRARY_DIALOG_ID: ViewId = ViewId::new("library_dialog");
const LIBRARY_BUTTON_ID: ViewId = ViewId::new("library");
const LIBRARY_LIST_ID: ViewId = ViewId::new("lib_list");
const LIBRARY_READ_BUTTON_ID: ViewId = ViewId::new("library_read");
const LIBRARY_CLOSE_BUTTON_ID: ViewId = ViewId::new("library_close");
const SETTINGS_BUTTON_ID: ViewId = ViewId::new("settings");
const BATTERY_BUTTON_ID: ViewId = ViewId::new("battery");
const BATTERY_DIALOG_ID: ViewId = ViewId::new("battery_dialog");
const BATTERY_CLOSE_ID: ViewId = ViewId::new("battery_close");
const ERROR_DIALOG_ID: ViewId = ViewId::new("error_dialog");
const LOADING_DIALOG_ID: ViewId = ViewId::new("loading_dialog");
const FAST_SCROLL_PANEL_ID: ViewId = ViewId::new("fast_scroll_panel");
const FAST_SCROLL_W: i32 = 300;
const FAST_SCROLL_H: i32 = 80;
const ORIENTATION_ID: ViewId = ViewId::new("orientation");
const BACKLIGHT_ID: ViewId = ViewId::new("backlight");
const FONT_SIZE_ID: ViewId = ViewId::new("font_size");
const DEEP_CLEAN_ID: ViewId = ViewId::new("deep_clean");
const PREV_PAGE_ID: ViewId = ViewId::new("prev_page");
const NEXT_PAGE_ID: ViewId = ViewId::new("next_page");
const SYNC_TIME_BUTTON_ID: ViewId = ViewId::new("sync_time");

const UI_FONT_SIZE_SMALL: f32 = 16.0;
const UI_FONT_SIZE_MEDIUM: f32 = 20.0;
const UI_FONT_SIZE_LARGE: f32 = 24.0;

static FONT_BYTES: &[u8] = include_bytes!("../fonts/AtkinsonHyperlegible-Regular.ttf");
static FONT_BOLD_BYTES: &[u8] = include_bytes!("../fonts/AtkinsonHyperlegible-Bold.ttf");
static BODY_FONT_BYTES: &[u8] = include_bytes!("../fonts/NoticiaText-Regular.ttf");
static BODY_BOLD_FONT_BYTES: &[u8] = include_bytes!("../fonts/NoticiaText-Bold.ttf");
static BODY_ITALIC_FONT_BYTES: &[u8] = include_bytes!("../fonts/NoticiaText-Italic.ttf");

fn handle_click(event: &mut GuiEvent) {
    if event.target == &ViewId::new("settings") {
        event.scene.show_view(&SETTINGS_DIALOG_ID);
    } else if event.target == &ViewId::new("dialog_close") {
        event.scene.hide_view(&SETTINGS_DIALOG_ID);
    } else if event.target == &LIBRARY_BUTTON_ID {
        event.scene.show_view(&LIBRARY_DIALOG_ID);
    } else if event.target == &LIBRARY_CLOSE_BUTTON_ID {
        event.scene.hide_view(&LIBRARY_DIALOG_ID);
    } else if event.target == &BATTERY_BUTTON_ID {
        event.scene.show_view(&BATTERY_DIALOG_ID);
    } else if event.target == &BATTERY_CLOSE_ID {
        event.scene.hide_view(&BATTERY_DIALOG_ID);
    } else if event.target == &ViewId::new("error_dismiss") {
        event.scene.hide_view(&ERROR_DIALOG_ID);
        event.scene.mark_dirty_all();
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

fn show_loading_dialog(scene: &mut Scene, filename: &str) {
    if let Some(v) = scene.get_view_mut(&ViewId::new("loading_msg")) {
        v.title = format!("Loading {filename}\u{2026}");
    }
    scene.show_view(&LOADING_DIALOG_ID);
    scene.mark_layout_dirty_view(&LOADING_DIALOG_ID);
}

fn hide_loading_dialog(scene: &mut Scene) {
    scene.hide_view(&LOADING_DIALOG_ID);
    scene.mark_dirty_view(&LOADING_DIALOG_ID);
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

fn make_scene(fonts: AppFonts, w: i32, h: i32) -> Scene {
    let mut scene = Scene::new_with_bounds(Bounds::new(0, 0, w, h));
    scene.set_focus_enabled(false);
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
        scene.add_view_to_parent(make_button(&LIBRARY_BUTTON_ID, "Library"), &topbar_id);
        scene.add_view_to_parent(h_spacer::make_h_spacer(&ViewId::new("spacer1")), &topbar_id);
        scene.add_view_to_parent(make_label(&ViewId::new("time"), "--:-- --"), &topbar_id);
        scene.add_view_to_parent(make_button(&BATTERY_BUTTON_ID, "85%"), &topbar_id);
        scene.add_view_to_parent( make_full_button(&SETTINGS_BUTTON_ID, "Settings", "settings", false),  &topbar_id );
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
                fonts,
                heading_font_px: 22.0 * 1.4,
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
        scene.add_view_to_parent(make_button(&PREV_PAGE_ID, "< Prev"), &bottombar_id);
        scene.add_view_to_parent(
            truncating_label::make_truncating_label(&ViewId::new("booktitle"), "Sherlock Holmes"),
            &bottombar_id,
        );

        scene.add_view_to_parent(
            make_label(&ViewId::new("chapter"), "Loading..."),
            &bottombar_id,
        );
        scene.add_view_to_parent(make_label(&ViewId::new("page"), ""), &bottombar_id);
        scene.add_view_to_parent(make_button(&NEXT_PAGE_ID, "Next >"), &bottombar_id);
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
        let settings_panel = make_panel(&SETTINGS_DIALOG_ID)
            .with_layout(Some(layout_centered_dialog))
            .with_h_flex(Fixed)
            .with_v_flex(Shrink)
            .with_size(440, 0)
            .with_visible(false)
            .with_state(Some(Box::new(PanelState {
                border_visible: true,
                gap: 8,
                padding: Insets::new_same(8),
            })));
        // ── Settings dialog (hidden, drawn last so it appears on top) ────────────
        scene.add_view_to_parent(
            make_label(&ViewId::new("dlg_title"), "Settings"),
            &SETTINGS_DIALOG_ID,
        );
        scene.add_view_to_parent(
            make_label(&ViewId::new("dlg_font_lbl"), "Font Size"),
            &SETTINGS_DIALOG_ID,
        );
        scene.add_view_to_parent(
            make_toggle_group(&FONT_SIZE_ID, vec!["Small", "Medium", "Large"], 1),
            &SETTINGS_DIALOG_ID,
        );
        scene.add_view_to_parent(
            make_label(&ViewId::new("dlg_bl_lbl"), "Backlight"),
            &SETTINGS_DIALOG_ID,
        );
        scene.add_view_to_parent(
            make_toggle_group(&BACKLIGHT_ID, vec!["Off", "Low", "High"], 2),
            &SETTINGS_DIALOG_ID,
        );
        scene.add_view_to_parent(
            make_label(&ViewId::new("dlg_orient_lbl"), "Orientation"),
            &SETTINGS_DIALOG_ID,
        );
        scene.add_view_to_parent(
            make_toggle_group(&ORIENTATION_ID, vec!["Port", "Land", "R.Port", "R.Land"], 0),
            &SETTINGS_DIALOG_ID,
        );
        scene.add_view_to_parent(
            make_button(&ViewId::new("sync_time"), "Sync Time"),
            &SETTINGS_DIALOG_ID,
        );
        scene.add_view_to_parent(make_button(&DEEP_CLEAN_ID, "Clean Screen"), &SETTINGS_DIALOG_ID);
        scene.add_view_to_parent(
            make_label(&ViewId::new("dlg_battery"), "Battery: 85%  (Charging)"),
            &SETTINGS_DIALOG_ID,
        );
        scene.add_view_to_parent(
            make_button(&ViewId::new("dialog_close"), "Close"),
            &SETTINGS_DIALOG_ID,
        );
        scene.add_view_to_root(settings_panel);
    }

    // ── Library dialog (hidden, shown when Library button pressed) ───────────
    {
        let lib_panel = make_panel(&LIBRARY_DIALOG_ID)
            .with_layout(Some(layout_centered_dialog))
            .with_h_flex(Fixed)
            .with_v_flex(Fixed)
            .with_size(440, 400)
            .with_visible(false)
            .with_state(Some(Box::new(PanelState {
                border_visible: true,
                gap: 8,
                padding: Insets::new_same(8),
            })));
        scene.add_view_to_parent(
            make_label(&ViewId::new("lib_title"), "Library"),
            &LIBRARY_DIALOG_ID,
        );
        scene.add_view_to_parent(
            make_list_view(&LIBRARY_LIST_ID, vec![], 0)
                .with_h_flex(Flex::Grow)
                .with_v_flex(Flex::Grow)
                .with_h_align(Align::Start),
            &LIBRARY_DIALOG_ID,
        );
        let lib_btn_row_id = ViewId::new("lib_btn_row");
        scene.add_view_to_parent(
            make_button(&LIBRARY_CLOSE_BUTTON_ID, "Cancel"),
            &lib_btn_row_id,
        );
        scene.add_view_to_parent(
            make_button(&LIBRARY_READ_BUTTON_ID, "Read"),
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

    // ── Loading dialog (hidden, shown while a book file is being read) ────────
    {
        let loading_panel = make_panel(&LOADING_DIALOG_ID)
            .with_layout(Some(layout_centered_dialog))
            .with_h_flex(Fixed)
            .with_v_flex(Flex::Shrink)
            .with_size(320, 0)
            .with_visible(false)
            .with_state(Some(Box::new(PanelState {
                border_visible: true,
                gap: 8,
                padding: Insets::new_same(16),
            })));
        scene.add_view_to_parent(
            make_label(&ViewId::new("loading_msg"), ""),
            &LOADING_DIALOG_ID,
        );
        scene.add_view_to_root(loading_panel);
    }

    // ── Battery dialog (hidden; shown when battery button tapped) ─────────────
    {
        let batt_panel = make_panel(&BATTERY_DIALOG_ID)
            .with_layout(Some(layout_centered_dialog))
            .with_h_flex(Fixed)
            .with_v_flex(Flex::Shrink)
            .with_size(320, 0)
            .with_visible(false)
            .with_state(Some(Box::new(PanelState {
                border_visible: true,
                gap: 8,
                padding: Insets::new_same(12),
            })));
        scene.add_view_to_parent(
            make_label(&ViewId::new("batt_title"), "Battery"),
            &BATTERY_DIALOG_ID,
        );
        scene.add_view_to_parent(
            make_label(&ViewId::new("batt_percent"), "Charge: 85%"),
            &BATTERY_DIALOG_ID,
        );
        scene.add_view_to_parent(
            make_label(&ViewId::new("batt_voltage"), "Voltage: 4050 mV"),
            &BATTERY_DIALOG_ID,
        );
        scene.add_view_to_parent(
            make_label(&ViewId::new("batt_status"), "Status: Not charging"),
            &BATTERY_DIALOG_ID,
        );
        scene.add_view_to_parent(
            make_button(&BATTERY_CLOSE_ID, "Dismiss"),
            &BATTERY_DIALOG_ID,
        );
        scene.add_view_to_root(batt_panel);
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

/// Parse all fonts and leak them into `'static` memory.
/// Works on both std (simulator) and no_std+alloc (ESP) since both provide `Box`.
fn load_fonts() -> AppFonts {
    let ui = Box::leak(Box::new(
        fontdue::Font::from_bytes(FONT_BYTES, fontdue::FontSettings::default())
            .expect("AtkinsonHyperlegible-Regular parse failed"),
    ));
    let ui_bold = Box::leak(Box::new(
        fontdue::Font::from_bytes(FONT_BOLD_BYTES, fontdue::FontSettings::default())
            .expect("AtkinsonHyperlegible-Bold parse failed"),
    ));
    let body = Box::leak(Box::new(
        fontdue::Font::from_bytes(BODY_FONT_BYTES, fontdue::FontSettings::default())
            .expect("NoticiaText-Regular parse failed"),
    ));
    let body_bold = Box::leak(Box::new(
        fontdue::Font::from_bytes(BODY_BOLD_FONT_BYTES, fontdue::FontSettings::default())
            .expect("NoticiaText-Bold parse failed"),
    ));
    let body_italic = Box::leak(Box::new(
        fontdue::Font::from_bytes(BODY_ITALIC_FONT_BYTES, fontdue::FontSettings::default())
            .expect("NoticiaText-Italic parse failed"),
    ));
    AppFonts { ui, ui_bold, body, body_bold, body_italic }
}

fn make_theme(fonts: &AppFonts) -> Theme {
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
            font: fonts.ui,
        },
        bold_font: FontKind::TrueType {
            size: UI_FONT_SIZE_MEDIUM,
            font: fonts.ui_bold,
        },
    }
}

fn load_book(state: &mut AppState, hw: &mut dyn HardwareAccess, filename: &String) {
    if let Some(data) = hw.load_book_file(&filename) {
        hw.save_bookmark(
            &state.current_filename,
            state.session.chapter_idx,
            state.session.reader.anchor_byte,
        );
        let new_book = book_from_data(&filename, data);
        state.cfg = cfg_from_scene(&mut state.scene, &state.theme, &state.fonts, hw.font_size());
        let new_session = match hw.load_bookmark(&filename) {
            Some((ch_idx, anchor)) => {
                BookSession::restore(new_book.as_ref(), &state.cfg, ch_idx, anchor)
                    .or_else(|_| BookSession::new(new_book.as_ref(), &state.cfg))
            }
            None => BookSession::new(new_book.as_ref(), &state.cfg),
        };
        hide_loading_dialog(&mut state.scene);
        if let Ok(s) = new_session {
            state.current_filename = filename.clone();
            state.session = s;
            state.book = new_book;
            state.update_content(hw);
            if let Some(v) = state.scene.get_view_mut(&ViewId::new("booktitle")) {
                v.title = filename.clone();
            }
        } else {
            show_error_dialog(&mut state.scene, &filename);
        }
    } else {
        hide_loading_dialog(&mut state.scene);
        show_error_dialog(&mut state.scene, &filename);
    }
}

#[cfg(feature = "simulator")]
fn main() {
    use embedded_graphics::geometry::Size;
    use embedded_graphics_simulator::{
        sdl2::Keycode, OutputSettingsBuilder, SimulatorDisplay, SimulatorEvent, Window,
    };
    use ereader::appstate::AppState;

    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let mut hw = SimHardware::new();
    let (mut win_w, mut win_h) = hw.orientation().logical_size();

    let mut display: SimulatorDisplay<Rgb565> =
        SimulatorDisplay::new(Size::new(win_w as u32, win_h as u32));
    let settings = OutputSettingsBuilder::new().scale(1).build();
    let mut window = Window::new("ereader_ui", &settings);

    let handlers: Vec<Callback> = vec![handle_click];
    let mut state: AppState = init_app_state(&hw);

    state.update_content(&hw);

    // Fast-scroll hold state (Up/Down arrow keys).

    let mut fast = FastPaging {
        fs_active: false,
        fs_target: 0usize,
        fs_last_step: Instant::now(),
        fs_pressed_at: None,
        forward: false,
    };

    state.scene.mark_layout_dirty();
    'sim_running: loop {
        // Advance fast-scroll page counter while a direction key is held.
        fast.handle_update_label(&mut state);

        if (!state.scene.dirty_rect.is_empty()) {
            info!("clip rect {}", state.scene.dirty_rect);
            let dirty = state.scene.dirty_rect.clone();
            let mut ctx = EmbeddedDrawingContext::new(&mut display);
            ctx.clip = dirty.clone();
            layout_scene(&mut state.scene, &state.theme);
            draw_scene(&mut state.scene, &mut ctx, &state.theme);
            window.update(&display);
        }

        let events: Vec<_> = window.events().collect();
        for event in events {
            match event {
                SimulatorEvent::Quit => break 'sim_running,
                // Keyboard shortcuts: arrow keys / Space simulate physical buttons.
                // Hold ≥1 s → fast-paging dialog; short press → regular nav on key-up.
                SimulatorEvent::KeyDown {
                    keycode: Keycode::Left,
                    repeat: false,
                    ..
                }
                | SimulatorEvent::KeyDown {
                    keycode: Keycode::Backspace,
                    repeat: false,
                    ..
                }
                | SimulatorEvent::KeyDown {
                    keycode: Keycode::Up,
                    repeat: false,
                    ..
                } => {
                    fast.start_backward();
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
                }
                | SimulatorEvent::KeyDown {
                    keycode: Keycode::Down,
                    repeat: false,
                    ..
                } => {
                    fast.start_forward();
                }
                SimulatorEvent::KeyUp {
                    keycode: Keycode::Left,
                    ..
                }
                | SimulatorEvent::KeyUp {
                    keycode: Keycode::Backspace,
                    ..
                }
                | SimulatorEvent::KeyUp {
                    keycode: Keycode::Up,
                    ..
                } => {
                    fast.end(&mut state, &mut hw);
                }
                SimulatorEvent::KeyUp {
                    keycode: Keycode::Right,
                    ..
                }
                | SimulatorEvent::KeyUp {
                    keycode: Keycode::Space,
                    ..
                }
                | SimulatorEvent::KeyUp {
                    keycode: Keycode::Down,
                    ..
                } => {
                    fast.end(&mut state, &mut hw);
                }
                SimulatorEvent::MouseButtonUp { point, .. } => {
                    if let Some(input) =
                        click_at(&mut state.scene, &handlers, GPoint::new(point.x, point.y))
                    {
                        if let Some(OutputAction::Command(ref cmd)) = input.action {
                            if input.source == ORIENTATION_ID {
                                hw.set_orientation(Orientation::from_cmd(cmd.as_str()));
                                let (new_w, new_h) = hw.orientation().logical_size();
                                if new_w != win_w || new_h != win_h {
                                    win_w = new_w;
                                    win_h = new_h;
                                    state.scene.resize(Bounds::new(0, 0, win_w, win_h));
                                    display = SimulatorDisplay::new(Size::new(
                                        win_w as u32,
                                        win_h as u32,
                                    ));
                                    window = Window::new("ereader_ui", &settings);
                                    state.cfg = cfg_from_scene(
                                        &mut state.scene,
                                        &state.theme,
                                        &state.fonts,
                                        hw.font_size(),
                                    );
                                    state.session.reader.relayout(&state.cfg);
                                    state.update_content(&hw);
                                }
                            } else if input.source == FONT_SIZE_ID {
                                hw.set_font_size(FontSize::from_cmd(cmd.as_str()));
                                state.cfg = cfg_from_scene(
                                    &mut state.scene,
                                    &state.theme,
                                    &state.fonts,
                                    hw.font_size(),
                                );
                                state.session.reader.relayout(&state.cfg);
                                state.update_content(&hw);
                            } else if input.source == BACKLIGHT_ID {
                                handle_backlight_action(cmd, &mut hw);
                            }
                        }
                        handle_click_action(&mut hw, &input, &mut state);
                        if input.source == DEEP_CLEAN_ID {
                            // no-op in simulator
                        } else if input.source == LIBRARY_READ_BUTTON_ID {
                            let filename = state
                                .scene
                                .get_view_mut(&LIBRARY_LIST_ID)
                                .and_then(|v| v.get_state::<ListState>())
                                .and_then(|s| s.items.get(s.selected).cloned());
                            if let Some(filename) = filename {
                                state.scene.hide_view(&LIBRARY_DIALOG_ID);
                                show_loading_dialog(&mut state.scene, &filename);
                                // Flush the loading screen to the window before blocking.
                                {
                                    let dirty = state.scene.dirty_rect.clone();
                                    let mut ctx = EmbeddedDrawingContext::new(&mut display);
                                    ctx.clip = dirty;
                                    layout_scene(&mut state.scene, &state.theme);
                                    draw_scene(&mut state.scene, &mut ctx, &state.theme);
                                    window.update(&display);
                                }
                                load_book(&mut state, &mut hw, &filename);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

fn init_app_state(hw: &dyn HardwareAccess) -> AppState {
    let (win_w, win_h) = hw.orientation().logical_size();
    let fonts = load_fonts();
    let mut pre_scene = make_scene(fonts, win_w, win_h);
    let theme = make_theme(&fonts);

    let pre_cfg = cfg_from_scene(&mut pre_scene, &theme, &fonts, hw.font_size());
    let pre_book = Box::new(HtmlBook::from_vec(WELCOME_HTML.to_vec()));
    let pre_session =
        BookSession::new(pre_book.as_ref(), &pre_cfg).expect("BookSession init failed");

    AppState {
        partial_refresh_count: 0,
        current_filename: String::from("__welcome__"),
        last_interaction: Instant::now(),
        cfg: pre_cfg,
        book: pre_book,
        session: pre_session,
        scene: pre_scene,
        fonts,
        theme,
    }
}

fn update_battery_labels(scene: &mut Scene, hw: &dyn HardwareAccess) {
    let info = hw.battery_info();
    let status = if info.is_charging {
        "Charging"
    } else {
        "Not charging"
    };
    if let Some(v) = scene.get_view_mut(&BATTERY_BUTTON_ID) {
        v.title = format!("{}%", info.percent);
    }
    if let Some(v) = scene.get_view_mut(&ViewId::new("batt_percent")) {
        v.title = format!("Charge: {}%", info.percent);
    }
    if let Some(v) = scene.get_view_mut(&ViewId::new("batt_voltage")) {
        v.title = format!("Voltage: {} mV", info.voltage_mv);
    }
    if let Some(v) = scene.get_view_mut(&ViewId::new("batt_status")) {
        v.title = format!("Status: {}", status);
    }
}

fn handle_click_action(hw: &mut dyn HardwareAccess, input: &InputResult, state: &mut AppState) {
    if input.source == BATTERY_BUTTON_ID {
        update_battery_labels(&mut state.scene, hw);
        state.scene.mark_layout_dirty_view(&BATTERY_DIALOG_ID);
        state.last_interaction = Instant::now();
    }
    if input.source == SYNC_TIME_BUTTON_ID {
        let t = hw.current_time_secs();
        if let Some(view) = state.scene.get_view_mut(&ViewId::new("time")) {
            view.title = format_time_utc(t);
        }
    }
    if input.source == PREV_PAGE_ID {
        state.nav_prev_page(hw);
        hw.save_bookmark(
            &state.current_filename,
            state.session.chapter_idx,
            state.session.reader.anchor_byte,
        );
        state.last_interaction = Instant::now();
    }
    if input.source == NEXT_PAGE_ID {
        state.nav_next_page(hw);
        hw.save_bookmark(
            &state.current_filename,
            state.session.chapter_idx,
            state.session.reader.anchor_byte,
        );
        state.last_interaction = Instant::now();
    }
    if input.source == LIBRARY_BUTTON_ID {
        let files = hw.list_book_files();
        if let Some(v) = state.scene.get_view_mut(&LIBRARY_LIST_ID) {
            if let Some(s) = v.get_state::<ListState>() {
                s.items = files;
                s.selected = 0;
            }
        }
        state.scene.mark_layout_dirty_view(&LIBRARY_DIALOG_ID);
        state.last_interaction = Instant::now();
    }
    if input.source == LIBRARY_LIST_ID {
        state.last_interaction = Instant::now();
    }
}

fn handle_backlight_action(cmd: &String, hw: &mut dyn HardwareAccess) {
    hw.set_backlight_level(BacklightLevel::from_cmd(cmd.as_str()));
    hw.save_settings();
}

fn calc_font_size(font_size: FontSize) -> f32 {
    match font_size {
        FontSize::Small => UI_FONT_SIZE_SMALL,
        FontSize::Medium => UI_FONT_SIZE_MEDIUM,
        FontSize::Large => UI_FONT_SIZE_LARGE,
    }
}

// ── ESP path ──────────────────────────────────────────────────────────────────
#[cfg(feature = "esp")]
use esp_backtrace as _;

#[cfg(feature = "esp")]
esp_bootloader_esp_idf::esp_app_desc!();

#[cfg(feature = "esp")]
use ereader::driver::display::{Display, DrawMode, Rectangle};
#[cfg(feature = "esp")]
use ereader::driver::gt911::GT911_ADDR_PRIMARY;
#[cfg(feature = "esp")]
use ereader::driver::Gt911;

// Light sleep after 60 s of inactivity; deep sleep after 60 min.
#[cfg(feature = "esp")]
const LIGHT_SLEEP_AFTER_SECS: u64 = 60;
#[cfg(feature = "esp")]
const DEEP_SLEEP_AFTER_SECS: u64 = 3600;

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

    /// Apply `mode` waveform to the logical dirty rect only.
    /// Converts to physical coords and calls `display.flush_region` with column masking,
    /// so pixels outside the rect receive VCOM (no drive) and are physically unchanged.
    fn flush_region(&mut self, dirty_rect: &Bounds, mode: DrawMode) {
        let lw = self.orientation.logical_size().0 as u16;
        let lh = self.orientation.logical_size().1 as u16;
        let lx1 = (dirty_rect.position.x.max(0) as u16).min(lw);
        let ly1 = (dirty_rect.position.y.max(0) as u16).min(lh);
        let lx2 = ((dirty_rect.position.x + dirty_rect.size.w as i32).max(0) as u16).min(lw);
        let ly2 = ((dirty_rect.position.y + dirty_rect.size.h as i32).max(0) as u16).min(lh);
        let corners = [
            self.orientation.logical_to_phys(lx1, ly1),
            self.orientation.logical_to_phys(lx1, ly2.saturating_sub(1)),
            self.orientation.logical_to_phys(lx2.saturating_sub(1), ly1),
            self.orientation
                .logical_to_phys(lx2.saturating_sub(1), ly2.saturating_sub(1)),
        ];
        let px = corners.iter().map(|c| c.0).min().unwrap_or(0);
        let py = corners.iter().map(|c| c.1).min().unwrap_or(0);
        let px2 = corners.iter().map(|c| c.0).max().unwrap_or(0);
        let py2 = corners.iter().map(|c| c.1).max().unwrap_or(0);
        let area = Rectangle {
            x: px,
            y: py,
            width: px2 - px + 1,
            height: py2 - py + 1,
        };
        self.display.flush_region(area, mode).unwrap();
    }

    fn clearing_flush_region(&mut self, dirty_rect: &Bounds) {
        self.flush_region(dirty_rect, DrawMode::WhiteOnBlack);
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
use ereader::appstate::{book_from_data, cfg_from_scene, AppState};
use ereader::bookview::{draw_book_content, layout_cfg, update_content, BookState, CONTENT_ID};
#[cfg(feature = "esp")]
use ereader::hardware::rtc_store_read;
use ereader::{h_spacer, truncating_label};
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
use iris_ui::input::{InputResult, OutputAction};
use iris_ui::panel::{make_panel, PanelState};
use iris_ui::view::Flex::Grow;
use log::info;
#[cfg(feature = "esp")]
use static_cell::StaticCell;
use Flex::{Fixed, Shrink};

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
    let handlers = vec![handle_click as Callback];
    let mut was_touching = false;

    // Counts consecutive partial refreshes so we can force a full ghost-clear
    // periodically.  E-paper capacitive field coupling from repeated partial
    // waveform passes slowly darkens white areas adjacent to the dirty rect;
    // a full refresh resets all pixels to a clean state.
    let mut state: AppState = init_app_state(&hw);

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
    sync_settings_ui(&mut state.scene, font_idx, bl_idx, ori_idx);

    // On sleep wakeup, try to reopen the SD card book the user was reading.
    // The filename was saved to NVS just before entering deep sleep.
    if is_sleep_wakeup {
        if let Some(last_file) = load_last_filename() {
            show_loading_dialog(&mut state.scene, &last_file);
            {
                let dirty = state.scene.dirty_rect.clone();
                bridge.clearing_flush_region(&dirty);
                {
                    let mut ctx = EmbeddedDrawingContext::new(&mut bridge);
                    ctx.clip = dirty.clone();
                    layout_scene(&mut state.scene, &state.theme);
                    draw_scene(&mut state.scene, &mut ctx, &state.theme);
                }
                bridge.flush_region(&dirty, DrawMode::BlackOnWhite);
            }
            if let Some(data) = hw.load_book_file(&last_file) {
                state.book = book_from_data(&last_file, data);
                state.current_filename = last_file;
            }
            hide_loading_dialog(&mut state.scene);
        }
    }

    state.session = if saved_chapter > 0 || saved_anchor > 0 {
        BookSession::restore(state.book.as_ref(), &state.cfg, saved_chapter, saved_anchor)
            .unwrap_or_else(|_| {
                BookSession::new(state.book.as_ref(), &state.cfg).expect(
                    "epub \
            load",
                )
            })
    } else {
        BookSession::new(state.book.as_ref(), &state.cfg).expect("epub load")
    };
    state.update_content(&hw);

    let mut just_woke = false;
    const PARTIAL_REFRESH_FULL_INTERVAL: u32 = 8;

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
                    if let Some(view) = state.scene.get_view_mut(&ViewId::new("time")) {
                        view.title = time_str.clone();
                    }
                    state.scene.mark_layout_dirty();
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

    let mut fast = FastPaging {
        fs_active: false,
        fs_target: 0usize,
        fs_last_step: Instant::now(),
        fs_pressed_at: None,
        forward: false,
    };

    'esp_running: loop {
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
            if !just_woke {
                if forward {
                    fast.start_forward();
                } else {
                    fast.start_backward();
                }
            }
            loop {
                let still_held = if forward {
                    hw.button_next_pressed()
                } else {
                    hw.button_prev_pressed()
                };
                if !still_held {
                    break;
                }

                fast.handle_update_label(&mut state);

                // Redraw only the panel while held (partial refresh).
                let dirty_rect = state.scene.dirty_rect.clone();
                if !dirty_rect.is_empty() {
                    let (sw, sh) = hw.orientation().logical_size();
                    let needs_full = dirty_rect.size.w >= sw && dirty_rect.size.h >= sh;
                    let force_full =
                        !needs_full && state.partial_refresh_count >= PARTIAL_REFRESH_FULL_INTERVAL;
                    if needs_full || force_full {
                        bridge.display.fill(0x0F).unwrap();
                        bridge.display.flush(DrawMode::WhiteOnBlack).unwrap();
                        state.partial_refresh_count = 0;
                    } else {
                        bridge.clearing_flush_region(&dirty_rect);
                        state.partial_refresh_count += 1;
                    }
                    {
                        let mut ctx = EmbeddedDrawingContext::new(&mut bridge);
                        ctx.clip = if force_full {
                            Bounds::new(0, 0, sw, sh)
                        } else {
                            dirty_rect
                        };
                        layout_scene(&mut state.scene, &state.theme);
                        draw_scene(&mut state.scene, &mut ctx, &state.theme);
                    }
                    if needs_full || force_full {
                        bridge.flush();
                    } else {
                        bridge.flush_region(&dirty_rect, DrawMode::BlackOnWhite);
                    }
                }

                EmbassyTimer::after(Duration::from_millis(10)).await;
            }

            if just_woke {
                // First press after light sleep: consume as wake-only, no page turn.
                fast.cancel();
            } else {
                fast.end(&mut state, &mut hw);
            }
            just_woke = false;
            hw.save_bookmark(
                &state.current_filename,
                state.session.chapter_idx,
                state.session.reader.anchor_byte,
            );
            state.last_interaction = Instant::now();
        }

        let dirty_rect = state.scene.dirty_rect.clone();
        let was_dirty = !dirty_rect.is_empty();
        let (scene_w, scene_h) = hw.orientation().logical_size();
        let needs_full_refresh = dirty_rect.size.w >= scene_w && dirty_rect.size.h >= scene_h;

        if was_dirty {
            info!("clip rect {}", state.scene.dirty_rect);
            // Ghost-clear pass: drives dark pixels to white so the BlackOnWhite draw can
            // correctly lighten any pixels that changed from dark to light (e.g. deselected
            // list item, dismissed dialog). Without this, white-target pixels get "no drive"
            // from the LUT and black display pixels stay black.
            //
            // Periodic full refresh: repeated partial waveform passes accumulate field
            // coupling that slowly darkens white areas adjacent to the dirty rect.
            // Every PARTIAL_REFRESH_FULL_INTERVAL partial refreshes we force a full clear.
            let force_full =
                !needs_full_refresh && state.partial_refresh_count >= PARTIAL_REFRESH_FULL_INTERVAL;
            if needs_full_refresh || force_full {
                bridge.display.fill(0x0F).unwrap();
                bridge.display.flush(DrawMode::WhiteOnBlack).unwrap();
                state.partial_refresh_count = 0;
            } else {
                bridge.clearing_flush_region(&dirty_rect);
                state.partial_refresh_count += 1;
            }
            {
                let mut ctx = EmbeddedDrawingContext::new(&mut bridge);
                ctx.clip = if force_full {
                    Bounds::new(0, 0, scene_w, scene_h)
                } else {
                    dirty_rect.clone()
                };
                layout_scene(&mut state.scene, &state.theme);
                draw_scene(&mut state.scene, &mut ctx, &state.theme);
            }
            if needs_full_refresh || force_full {
                bridge.flush();
            } else {
                bridge.flush_region(&dirty_rect, DrawMode::BlackOnWhite);
            }
        }

        if let Some((tx, ty)) = bridge.display.read_touch(&mut gt911) {
            if !was_touching {
                let (lx, ly) = hw.orientation().phys_to_logical(tx, ty);
                if let Some(input) = click_at(&mut state.scene, &handlers, GPoint::new(lx, ly)) {
                    if let Some(OutputAction::Command(ref cmd)) = input.action {
                        if input.source == FONT_SIZE_ID {
                            hw.set_font_size(FontSize::from_cmd(cmd.as_str()));
                            state.cfg = cfg_from_scene(
                                &mut state.scene,
                                &state.theme,
                                &state.fonts,
                                hw.font_size(),
                            );
                            state.session.reader.relayout(&state.cfg);
                            state.update_content(&hw);
                            hw.save_settings();
                        } else if input.source == BACKLIGHT_ID {
                            handle_backlight_action(cmd, &mut hw);
                        } else if input.source == ORIENTATION_ID {
                            hw.set_orientation(Orientation::from_cmd(cmd.as_str()));
                            bridge.orientation = hw.orientation();
                            let (new_w, new_h) = hw.orientation().logical_size();
                            state.scene.resize(Bounds::new(0, 0, new_w, new_h));
                            state.cfg = cfg_from_scene(
                                &mut state.scene,
                                &state.theme,
                                &state.fonts,
                                hw.font_size(),
                            );
                            state.session.reader.relayout(&state.cfg);
                            state.update_content(&hw);
                            hw.save_settings();
                        }
                    }
                    handle_click_action(&mut hw, &input, &mut state);
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
                    } else if input.source == DEEP_CLEAN_ID {
                        info!("deep clean started");
                        bridge.display.deep_clean(3).unwrap();
                        state.partial_refresh_count = 0;
                        state.scene.mark_dirty_all();
                    } else if input.source == LIBRARY_READ_BUTTON_ID {
                        let filename = state
                            .scene
                            .get_view_mut(&LIBRARY_LIST_ID)
                            .and_then(|v| v.get_state::<ListState>())
                            .and_then(|s| s.items.get(s.selected).cloned());
                        if let Some(filename) = filename {
                            state.scene.hide_view(&LIBRARY_DIALOG_ID);
                            show_loading_dialog(&mut state.scene, &filename);
                            // Flush the loading screen to e-paper before the blocking SD read.
                            // E-paper is bistable so the "Loading…" message stays visible
                            // for the full duration of the blocking file read.
                            {
                                let dirty = state.scene.dirty_rect.clone();
                                bridge.clearing_flush_region(&dirty);
                                {
                                    let mut ctx = EmbeddedDrawingContext::new(&mut bridge);
                                    ctx.clip = dirty.clone();
                                    layout_scene(&mut state.scene, &state.theme);
                                    draw_scene(&mut state.scene, &mut ctx, &state.theme);
                                }
                                bridge.flush_region(&dirty, DrawMode::BlackOnWhite);
                                state.partial_refresh_count += 1;
                            }
                            load_book(&mut state, &mut hw, &filename);
                        }
                        state.last_interaction = Instant::now();
                    } else {
                        state.last_interaction = Instant::now();
                    }
                }
            }
            was_touching = true;
        } else {
            was_touching = false;
        }

        // Two-tier inactivity sleep: light sleep at 60 s, deep sleep at 60 min.
        let elapsed_secs = state.last_interaction.elapsed().as_secs();
        if elapsed_secs >= DEEP_SLEEP_AFTER_SECS {
            log::info!("inactivity timeout — entering deep sleep");
            if let Some(v) = state.scene.get_view_mut(&ViewId::new("page")) {
                v.title = "Sleeping\u{2026} Press BOOT to wake".into();
            }
            state.scene.mark_dirty_all();
            // Render the sleep message before powering off.
            let sleep_dirty = state.scene.dirty_rect.clone();
            {
                let mut ctx = EmbeddedDrawingContext::new(&mut bridge);
                ctx.clip = sleep_dirty;
                layout_scene(&mut state.scene, &state.theme);
                draw_scene(&mut state.scene, &mut ctx, &state.theme);
            }
            bridge.flush();
            bridge.display.power_off();
            // Persist filename and position so wakeup can reopen the same book.
            save_last_filename(&state.current_filename);
            hw.save_bookmark(
                &state.current_filename,
                state.session.chapter_idx,
                state.session.reader.anchor_byte,
            );
            // On ESP: saves RTC state and enters deep sleep (never returns).
            // On simulator enter_deep_sleep is a no-op; reset the timer so we
            // don't loop immediately back into the sleep check.
            hw.enter_deep_sleep(state.session.chapter_idx, state.session.reader.anchor_byte);
            state.last_interaction = Instant::now();
            state.update_content(&hw);
        } else if elapsed_secs >= LIGHT_SLEEP_AFTER_SECS {
            log::info!("inactivity timeout — entering light sleep");
            // Backlight is turned off inside enter_light_sleep and restored on return.
            hw.enter_light_sleep();
            state.last_interaction = Instant::now();
            just_woke = true;
        }

        EmbassyTimer::after(Duration::from_millis(50)).await;
    }
}

struct FastPaging {
    fs_active: bool,
    fs_target: usize,
    fs_last_step: Instant,
    fs_pressed_at: Option<Instant>,
    forward: bool,
}

impl FastPaging {
    pub(crate) fn start_backward(&mut self) {
        self.forward = false;
        self.fs_pressed_at = Some(Instant::now());
    }
    pub(crate) fn start_forward(&mut self) {
        self.forward = true;
        self.fs_pressed_at = Some(Instant::now());
    }
    pub(crate) fn end(&mut self, state: &mut AppState, hw: &mut dyn HardwareAccess) {
        if self.fs_pressed_at.is_some() {
            if self.fs_active {
                state.session.reader.go_to_page(self.fs_target);
                state.update_content(hw);
                state.scene.hide_view(&FAST_SCROLL_PANEL_ID);
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
    pub(crate) fn cancel(&mut self) {
        self.fs_active = false;
        self.fs_pressed_at = None;
    }
    pub(crate) fn handle_update_label(&mut self, mut state: &mut AppState) {
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
// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(all(test, feature = "simulator"))]
mod tests {
    use super::*;
    use ereader::bookview::{layout_cfg, CONTENT_ID};
    use ereader::font::line_height;
    use ereader::hardware::FontSize;
    use iris_ui::scene::layout_scene;

    fn make_test_fonts() -> AppFonts {
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
        let fonts = make_test_fonts();
        let w = 960i32;
        let h = 540i32;

        let mut scene = make_scene(fonts, w, h);
        let theme = make_theme(&fonts);
        layout_scene(&mut scene, &theme);

        let content_bounds = scene
            .get_view_bounds(&CONTENT_ID)
            .expect("content view not in scene");
        let content_w = content_bounds.size.w;
        let content_h = content_bounds.size.h as u32;

        for font_size in [FontSize::Small, FontSize::Medium, FontSize::Large] {
            let cfg = layout_cfg(&fonts, font_size, content_w, content_h as i32);
            let render_pad_total = 24u32; // pad_y (12) top + pad_y (12) bottom
            let expected = content_h.saturating_sub(render_pad_total);
            assert_eq!(
                cfg.screen_height, expected,
                "FontSize::{:?}: layout uses {} px but render has {} px available ({} - {})",
                font_size, cfg.screen_height, expected, content_h, render_pad_total,
            );
        }
    }

    /// Each page's worth of lines as counted by the layout engine should all
    /// fit within the content view without clipping.
    #[test]
    fn page_line_count_fits_in_content_view() {
        let fonts = make_test_fonts();
        let w = 960i32;
        let h = 540i32;

        let mut scene = make_scene(fonts, w, h);
        let theme = make_theme(&fonts);
        layout_scene(&mut scene, &theme);

        let content_bounds = scene
            .get_view_bounds(&CONTENT_ID)
            .expect("content view not in scene");
        let content_w = content_bounds.size.w;
        let content_h = content_bounds.size.h as u32;
        let render_usable = content_h.saturating_sub(24); // top+bottom pad_y

        for font_size in [FontSize::Small, FontSize::Medium, FontSize::Large] {
            let cfg = layout_cfg(&fonts, font_size, content_w, content_h as i32);
            let font_px = ereader::font::font_px_for(font_size);
            let line_h = line_height(fonts.body, font_px) as u32 + 4; // matches render_ttf_text
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
