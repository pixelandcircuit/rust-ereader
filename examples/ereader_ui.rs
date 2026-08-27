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
use ereader::book::HtmlBook;
#[cfg(feature = "esp")]
use ereader::book::TxtBook;
use ereader::font::AppFonts;
#[cfg(feature = "simulator")]
use ereader::hardware::SimHardware;

#[cfg(feature = "simulator")]
use std::time::Instant;
#[cfg(feature = "simulator")]
use embedded_graphics_simulator::{SimulatorDisplay, Window};

#[cfg(feature = "esp")]
use ereader::hardware::{
    flash_write_task, load_cold_boot_position, load_last_filename, load_settings,
    save_before_sleep, EspHardware,
};
use ereader::hardware::{BacklightLevel, FontSize, HardwareAccess,  Orientation};
use ereader::reader::BookSession;
use iris_ui::button::{make_button, make_full_button};
use iris_ui::device::EmbeddedDrawingContext;
use iris_ui::geom::{Bounds, Insets, Point as GPoint};
use iris_ui::label::{make_header_label, make_label};
use iris_ui::layouts::{layout_centered_dialog, layout_hbox, layout_vbox};
use iris_ui::list_view::{make_list_view, ListState};
use iris_ui::scene::{draw_scene, layout_scene, pointer_down_at, pointer_up_at, Scene};
use iris_ui::toggle_group::{make_toggle_group, SelectOneOfState};
use iris_ui::view::{Align, Flex, View, ViewId};
use iris_ui::{Callback, DrawEvent, FontKind, GuiEvent, LayoutEvent, Theme, ViewStyle};
use Align::Center;

const WELCOME_HTML: &[u8] = include_bytes!("welcome.html");

const SETTINGS_DIALOG_ID: ViewId = ViewId::new("settings_dialog");
const LIBRARY_DIALOG_ID: ViewId = ViewId::new("library_dialog");
const LIBRARY_BUTTON_ID: ViewId = ViewId::new("library");
const TIME_LABEL_ID: ViewId = ViewId::new("time_label");
const LIBRARY_LIST_ID: ViewId = ViewId::new("lib_list");
const LIBRARY_READ_BUTTON_ID: ViewId = ViewId::new("library_read");
const LIBRARY_CLOSE_BUTTON_ID: ViewId = ViewId::new("library_close");
const SETTINGS_BUTTON_ID: ViewId = ViewId::new("settings");
const BATTERY_BUTTON_ID: ViewId = ViewId::new("battery");
const BATTERY_DIALOG_ID: ViewId = ViewId::new("battery_dialog");
const BATTERY_CLOSE_ID: ViewId = ViewId::new("battery_close");
const ERROR_DIALOG_ID: ViewId = ViewId::new("error_dialog");
const LOADING_DIALOG_ID: ViewId = ViewId::new("loading_dialog");
const LOADING_PROGRESS_BAR_ID: ViewId = ViewId::new("loading_progress_bar");
const SLEEP_DIALOG_ID: ViewId = ViewId::new("sleep_dialog");
const ORIENTATION_ID: ViewId = ViewId::new("orientation");
const BACKLIGHT_ID: ViewId = ViewId::new("backlight");
const FONT_SIZE_ID: ViewId = ViewId::new("font_size");
const DEEP_CLEAN_ID: ViewId = ViewId::new("deep_clean");
const DEEP_SLEEP_BUTTON_ID: ViewId = ViewId::new("deep_sleep");
const PREV_PAGE_ID: ViewId = ViewId::new("prev_page");
const NEXT_PAGE_ID: ViewId = ViewId::new("next_page");
const SYNC_TIME_BUTTON_ID: ViewId = ViewId::new("sync_time");
const TZ_MINUS_ID: ViewId = ViewId::new("tz_minus");
const TZ_PLUS_ID: ViewId = ViewId::new("tz_plus");
const TZ_LABEL_ID: ViewId = ViewId::new("tz_label");

const UI_FONT_SIZE: f32 = 20.0;

static FONT_BYTES: &[u8]             = include_bytes!("../fonts/AtkinsonHyperlegible-Regular.ttf");
static FONT_BOLD_BYTES: &[u8]        = include_bytes!("../fonts/AtkinsonHyperlegible-Bold.ttf");
static BODY_FONT_BYTES: &[u8]        = include_bytes!("../fonts/CrimsonText-Regular.ttf");
static BODY_FONT_BOLD_BYTES: &[u8]   = include_bytes!("../fonts/CrimsonText-Bold.ttf");
static BODY_FONT_ITALIC_BYTES: &[u8] = include_bytes!("../fonts/CrimsonText-Italic.ttf");

fn handle_click(event: &mut GuiEvent<Rgb565>) {
    // This handler runs for every pointer event dispatched to a hit target
    // (both PointerDown and PointerUp) — only act on release, or dialogs
    // would open/close the instant a button is pressed rather than tapped.
    if !matches!(event.event_type, InputEvent::PointerUp(_)) {
        return;
    }
    if event.target == &ViewId::new("settings") {
        event.scene.show_view(&SETTINGS_DIALOG_ID);
    } else if event.target == &ViewId::new("dialog_close") {
        event.scene.hide_view(&SETTINGS_DIALOG_ID);
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

fn show_error_dialog(scene: &mut Scene<Rgb565>, filename: &str) {
    if let Some(v) = scene.get_view_mut(&ViewId::new("err_msg")) {
        v.title = String::from(filename);
    }
    scene.show_view(&ERROR_DIALOG_ID);
    scene.mark_layout_dirty();
}

struct ProgressBarState {
    progress: u8, // 0–100
}

fn draw_progress_bar(e: &mut DrawEvent<Rgb565>) {
    // e.view.bounds is the view's own bounds in parent-local coordinates,
    // consistent with how draw_book_content uses it. e.bounds is the scene bounds.
    let bounds = e.view.bounds;
    let pct = e
        .view
        .get_state::<ProgressBarState>()
        .map(|s| s.progress)
        .unwrap_or(0);
    e.ctx.fill_rect(&bounds, &Rgb565::WHITE);
    if pct > 0 {
        let fill_w = (bounds.size.w as u32 * pct as u32 / 100) as i32;
        let fill = Bounds::new(bounds.position.x, bounds.position.y, fill_w, bounds.size.h);
        e.ctx.fill_rect(&fill, &Rgb565::BLACK);
    }
    e.ctx.stroke_rect(&bounds, &Rgb565::BLACK);
}

fn show_loading_dialog(scene: &mut Scene<Rgb565>, filename: &str) {
    if let Some(v) = scene.get_view_mut(&ViewId::new("loading_msg")) {
        v.title = format!("Loading {filename}\u{2026}");
    }
    set_loading_progress(scene, 0);
    scene.show_view(&LOADING_DIALOG_ID);
    // we need to trigger a relayout the first time a dialog is show, or if it's contents may have changed.
    scene.mark_layout_dirty_view(&LOADING_DIALOG_ID);
}

fn set_loading_progress(scene: &mut Scene<Rgb565>, pct: u8) {
    if let Some(v) = scene.get_view_mut(&LOADING_PROGRESS_BAR_ID) {
        if let Some(s) = v.get_state::<ProgressBarState>() {
            s.progress = pct;
            info!("set loading progress {}",s.progress);
        }
    }
    // Mark the whole dialog dirty — the progress bar's own bounds have w=0
    // before layout runs, so marking it alone produces an empty dirty rect.
    scene.mark_dirty_view(&LOADING_DIALOG_ID);
}

fn make_scene(fonts: AppFonts, w: i32, h: i32) -> Scene<Rgb565> {
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
        scene.add_view_to_parent(make_h_spacer(&ViewId::new("spacer1")), &topbar_id);
        scene.add_view_to_parent(make_label(&TIME_LABEL_ID, "--:-- --"), &topbar_id);
        scene.add_view_to_parent(make_button(&BATTERY_BUTTON_ID, "85%"), &topbar_id);
        scene.add_view_to_parent(
            make_full_button(&SETTINGS_BUTTON_ID, "Settings", "settings", false),
            &topbar_id,
        );
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
        fn fill_all_space(layout: &mut LayoutEvent<Rgb565>) {
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
            make_header_label(&ViewId::new("dlg_title"), "Settings").with_h_align(Start),
            &SETTINGS_DIALOG_ID,
        );
        scene.add_view_to_parent(
            make_label(&ViewId::new("dlg_font_lbl"), "Font Size").with_h_align(Start),
            &SETTINGS_DIALOG_ID,
        );
        scene.add_view_to_parent(
            make_toggle_group(&FONT_SIZE_ID, vec!["Small", "Medium", "Large"], 1),
            &SETTINGS_DIALOG_ID,
        );
        scene.add_view_to_parent(
            make_label(&ViewId::new("dlg_bl_lbl"), "Backlight").with_h_align(Start),
            &SETTINGS_DIALOG_ID,
        );
        scene.add_view_to_parent(
            make_toggle_group(&BACKLIGHT_ID, vec!["Off", "Low", "High"], 2),
            &SETTINGS_DIALOG_ID,
        );
        scene.add_view_to_parent(
            make_label(&ViewId::new("dlg_orient_lbl"), "Orientation").with_h_align(Start),
            &SETTINGS_DIALOG_ID,
        );
        scene.add_view_to_parent(
            make_toggle_group(&ORIENTATION_ID, vec!["Port", "Land", "R.Port", "R.Land"], 0),
            &SETTINGS_DIALOG_ID,
        );

        scene.add_view_to_parent(
            make_label(&ViewId::new("dlg_tz_lbl"), "Time Zone").with_h_align(Start),
            &SETTINGS_DIALOG_ID,
        );
        let tz_row = make_panel(&ViewId::new("tz_row"))
            .with_h_flex(Grow)
            .with_layout(Some(layout_hbox))
            .with_state(Some(Box::new(PanelState {
                border_visible: false,
                gap: 8,
                padding: Insets::new_same(0),
            })));
        scene.add_view_to_parent(make_button(&TZ_MINUS_ID, "−"), &tz_row.name);
        scene.add_view_to_parent(
            make_label(&TZ_LABEL_ID, "UTC").with_h_flex(Grow).with_h_align(Center),
            &tz_row.name,
        );
        scene.add_view_to_parent(make_button(&TZ_PLUS_ID, "+"), &tz_row.name);
        scene.add_view_to_parent(tz_row, &SETTINGS_DIALOG_ID);

        let row2 = make_panel(&ViewId::new("row2"))
            .with_h_flex(Grow)
            .with_layout(Some(layout_hbox))
            .with_state(Some(Box::new(PanelState {
                border_visible: false,
                gap: 8,
                padding: Insets::new_same(0),
            })));
        scene.add_view_to_parent(
            make_button(&ViewId::new("sync_time"), "Sync Time"),
            &row2.name,
        );
        scene.add_view_to_parent(make_button(&DEEP_CLEAN_ID, "Clean Screen"), &row2.name);
        scene.add_view_to_parent(make_button(&DEEP_SLEEP_BUTTON_ID, "Sleep Now"), &row2.name);
        scene.add_view_to_parent(row2, &SETTINGS_DIALOG_ID);

        let row3 = make_panel(&ViewId::new("row3"))
            .with_h_flex(Grow)
            .with_layout(Some(layout_hbox))
            .with_state(Some(Box::new(PanelState {
                border_visible: false,
                gap: 8,
                padding: Insets::new_same(0),
            })));

        scene.add_view_to_parent(make_h_spacer(&ViewId::new("spacer2")), &row3.name);
        scene.add_view_to_parent(
            make_full_button(&ViewId::new("dialog_close"), "Close", "close", true),
            &row3.name,
        );
        scene.add_view_to_parent(row3, &SETTINGS_DIALOG_ID);
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
            make_full_button(&LIBRARY_READ_BUTTON_ID, "Read","read",true),
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
        scene.add_view_to_parent(
            View::default()
                .with_name(LOADING_PROGRESS_BAR_ID)
                .with_h_flex(Flex::Grow)
                .with_v_flex(Flex::Fixed)
                .with_size(0, 20)
                .with_draw(Some(draw_progress_bar))
                .with_state(Some(Box::new(ProgressBarState { progress: 0 }))),
            &LOADING_DIALOG_ID,
        );
        scene.add_view_to_root(loading_panel);
    }

    // ── Sleep dialog (hidden, shown just before entering deep sleep) ─────────
    {
        let sleep_panel = make_panel(&SLEEP_DIALOG_ID)
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
            make_header_label(&ViewId::new("sleep_msg"), "Sleeping...").with_h_align(Center),
            &SLEEP_DIALOG_ID,
        );
        scene.add_view_to_parent(
            make_label(&ViewId::new("sleep_submsg"), "Press BOOT to wake").with_h_align(Center),
            &SLEEP_DIALOG_ID,
        );
        scene.add_view_to_root(sleep_panel);
    }

    // ── Battery dialog (hidden; shown when battery button tapped) ─────────────
    {
        let batt_panel = make_panel(&BATTERY_DIALOG_ID)
            .with_layout(Some(layout_centered_dialog))
            .with_h_flex(Fixed)
            .with_v_flex(Shrink)
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
            make_label(&ViewId::new("mem_psram"), "PSRAM free: --"),
            &BATTERY_DIALOG_ID,
        );
        scene.add_view_to_parent(
            make_label(&ViewId::new("mem_sram"), "SRAM free: --"),
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
            .with_layout(Some(layout_centered_dialog))
            .with_visible(false)
            .with_state(Some(Box::new(PanelState {
                border_visible: true,
                gap: 0,
                padding: Insets::new_same(0),
            })));
        scene.add_view_to_parent(
            make_label(&FAST_SCROLL_LABEL_ID, "Ch 1/1 Page 0 / 0"),
            &FAST_SCROLL_PANEL_ID,
        );
        scene.add_view_to_root(fs_panel);
    }

    scene
}

fn format_time_local(unix_secs: u64, utc_offset_minutes: i32) -> String {
    let local_secs = if utc_offset_minutes >= 0 {
        unix_secs.saturating_add(utc_offset_minutes as u64 * 60)
    } else {
        unix_secs.saturating_sub((-utc_offset_minutes) as u64 * 60)
    };
    let h24 = (local_secs / 3600) % 24;
    let m = (local_secs / 60) % 60;
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

fn format_tz_label(minutes: i32) -> String {
    if minutes == 0 {
        return String::from("UTC");
    }
    let sign = if minutes > 0 { '+' } else { '-' };
    let abs = minutes.abs();
    let h = abs / 60;
    let m = abs % 60;
    if m == 0 {
        format!("UTC{}{}", sign, h)
    } else {
        format!("UTC{}{}:{:02}", sign, h, m)
    }
}

/// Parse all fonts and leak them into `'static` memory.
/// Works on both std (simulator) and no_std+alloc (ESP) since both provide `Box`.
///
/// CrimsonText (~105 KB/variant) is used for body text — compact enough that all
/// three variants (Regular, Bold, Italic) fit in the 8 MB PSRAM alongside the two
/// Atkinson UI fonts.
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
            .expect("CrimsonText-Regular parse failed"),
    ));
    let body_bold = Box::leak(Box::new(
        fontdue::Font::from_bytes(BODY_FONT_BOLD_BYTES, fontdue::FontSettings::default())
            .expect("CrimsonText-Bold parse failed"),
    ));
    let body_italic = Box::leak(Box::new(
        fontdue::Font::from_bytes(BODY_FONT_ITALIC_BYTES, fontdue::FontSettings::default())
            .expect("CrimsonText-Italic parse failed"),
    ));
    AppFonts { ui, ui_bold, body, body_bold, body_italic }
}

fn make_theme(fonts: &AppFonts) -> Theme<Rgb565> {
    Theme {
        standard: ViewStyle {
            fill: Rgb565::WHITE,
            text: Rgb565::BLACK,
        },
        accented: ViewStyle {
            fill: Rgb565::BLACK,
            text: Rgb565::WHITE,
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
            size: UI_FONT_SIZE,
            font: fonts.ui,
        },
        bold_font: FontKind::TrueType {
            size: UI_FONT_SIZE,
            font: fonts.ui_bold,
        },
    }
}

#[cfg(feature = "simulator")]
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
        state.scene.hide_view(&LOADING_DIALOG_ID);
        if let Ok(s) = new_session {
            state.current_filename = filename.clone();
            hw.save_last_filename(&filename);
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
        state.scene.hide_view(&LOADING_DIALOG_ID);
        show_error_dialog(&mut state.scene, &filename);
    }
}

fn update_label(state: &mut AppState, label: &ViewId, value:String) {
    if let Some(v) = state.scene.get_view_mut(label) {
        v.title = value;
        state.scene.mark_dirty_view(label);
        info!("marked label dirty: {}", label);
    }
}

#[cfg(feature = "simulator")]
pub struct SimNativeScreen {
    pub size: Size,
    pub display: SimulatorDisplay<Rgb565>,
    pub settings: embedded_graphics_simulator::OutputSettings,
    pub window: Window,
}
#[cfg(feature = "simulator")]
impl SimNativeScreen {
}

#[cfg(feature = "simulator")]
impl NativeScreen for SimNativeScreen {
    fn resize(&mut self, size: Size) {
        self.display = SimulatorDisplay::new(size);
        self.window = Window::new("ereader_ui", &self.settings);
    }

    fn deep_clean(&mut self, state: &mut AppState) {
        // no-op in simulator
        self.refresh(state);
    }

    fn deep_sleep(&mut self, state: &mut AppState) {
        self.refresh(state);
    }


    fn refresh(&mut self, state: &mut AppState) {
        // Flush the loading screen to the window before blocking.
        if !state.scene.dirty_rect.is_empty() {
            info!("clip rect {}", state.scene.dirty_rect);
            let dirty = state.scene.dirty_rect.clone();
            let mut ctx = EmbeddedDrawingContext::new(&mut self.display);
            ctx.clip = dirty;
            layout_scene(&mut state.scene, &state.theme);
            draw_scene(&mut state.scene, &mut ctx, &state.theme);
            self.window.update(&self.display);
        }
    }
}


#[cfg(feature = "simulator")]
fn main() {
    use embedded_graphics::geometry::Size;
    use embedded_graphics_simulator::{
        sdl2::Keycode, OutputSettingsBuilder, SimulatorDisplay, SimulatorEvent, Window,
    };
    use ereader::appstate::AppState;
    use ereader::fast_paging::FastPaging;

    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let mut hw = SimHardware::new();
    let (win_wx, win_hx) = hw.orientation().logical_size();

    let settings = OutputSettingsBuilder::new().scale(1).build();
    let mut native_screen = SimNativeScreen {
        size: Size::new(win_wx as u32, win_hx as u32),
        display: SimulatorDisplay::new(Size::new(win_wx as u32, win_hx as u32)),
        settings,
        window: Window::new("ereader_ui", &settings),
    };

    let handlers: Vec<Callback<Rgb565>> = vec![handle_click];
    let mut state: AppState = init_app_state(&hw);

    state.update_content(&hw);

    // Fast-scroll hold state (Up/Down arrow keys).
    let mut fast = FastPaging::default();

    state.scene.mark_layout_dirty();
    'sim_running: loop {
        // Advance fast-scroll page counter while a direction key is held.
        fast.handle_update_label(&mut state);
        native_screen.refresh(&mut state);

        let events: Vec<_> = native_screen.window.events().collect();
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
                SimulatorEvent::MouseButtonDown { point, .. } => {
                    pointer_down_at(&mut state.scene, &handlers, GPoint::new(point.x, point.y));
                }
                SimulatorEvent::MouseButtonUp { point, .. } => {
                    if let Some(input) =
                        pointer_up_at(&mut state.scene, &handlers, GPoint::new(point.x, point.y))
                    {
                        if let Some(OutputAction::Command(ref cmd)) = input.action {
                            if input.source == ORIENTATION_ID {
                                hw.set_orientation(Orientation::from_cmd(cmd.as_str()));
                                let (new_w, new_h) = hw.orientation().logical_size();
                                let new_size = Size::new(new_w as u32, new_h as u32);
                                state.scene.resize(Bounds::new(0, 0, new_size.width as i32, new_size.height as i32));
                                native_screen.resize(new_size);
                                state.cfg = cfg_from_scene(
                                    &mut state.scene,
                                    &state.theme,
                                    &state.fonts,
                                    hw.font_size(),
                                );
                                state.session.reader.relayout(&state.cfg);
                                state.update_content(&hw);
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
                            native_screen.deep_clean(&mut state);
                        } else if input.source == DEEP_SLEEP_BUTTON_ID {
                            info!("deep sleep button pressed — showing sleep screen");
                            state.scene.hide_view(&SETTINGS_DIALOG_ID);
                            state.scene.show_view(&SLEEP_DIALOG_ID);
                            state.scene.mark_layout_dirty();
                            native_screen.refresh(&mut state);
                            // On simulator this is a no-op — there's no real hardware
                            // to power off, so the sleep screen just stays on screen.
                            hw.enter_deep_sleep(
                                state.session.chapter_idx,
                                state.session.reader.anchor_byte,
                            );
                        } else if input.source == LIBRARY_READ_BUTTON_ID {
                            let filename = state
                                .scene
                                .get_view_mut(&LIBRARY_LIST_ID)
                                .and_then(|v| v.get_state::<ListState>())
                                .and_then(|s| s.items.get(s.selected).cloned());
                            if let Some(filename) = filename {
                                state.scene.hide_view(&LIBRARY_DIALOG_ID);
                                show_loading_dialog(&mut state.scene, &filename);
                                native_screen.refresh(&mut state);
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
        full_quality_count: 0,
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

fn fmt_bytes(n: usize) -> String {
    if n >= 1024 * 1024 {
        format!("{:.1} MB", n as f32 / (1024.0 * 1024.0))
    } else {
        format!("{} KB", n / 1024)
    }
}

fn update_battery_labels(scene: &mut Scene<Rgb565>, hw: &dyn HardwareAccess) {
    let info = hw.battery_info();
    let status = if info.is_charging { "Charging" } else { "Not charging" };
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
    let mem = hw.memory_info();
    if let Some(v) = scene.get_view_mut(&ViewId::new("mem_psram")) {
        v.title = format!("PSRAM: {} / {} free", fmt_bytes(mem.psram_free_bytes), fmt_bytes(mem.psram_total_bytes));
    }
    if let Some(v) = scene.get_view_mut(&ViewId::new("mem_sram")) {
        v.title = format!("SRAM: {} / {} free", fmt_bytes(mem.sram_free_bytes), fmt_bytes(mem.sram_total_bytes));
    }
    scene.mark_layout_dirty_view(&BATTERY_DIALOG_ID);
}

fn handle_click_action(hw: &mut dyn HardwareAccess, input: &InputResult, state: &mut AppState) {
    if input.source == BATTERY_BUTTON_ID {
        update_battery_labels(&mut state.scene, hw);
        info!("marking battery dialog dirty");
    }
    if input.source == SYNC_TIME_BUTTON_ID {
        let t = hw.current_time_secs();
        let time_str = format_time_local(t, hw.utc_offset_minutes());
        update_label(state,&TIME_LABEL_ID, time_str);
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
        state.scene.show_view(&LIBRARY_DIALOG_ID);
        state.scene.mark_layout_dirty_view(&LIBRARY_DIALOG_ID);
        info!("marking the library dialog layout dirty");
        state.last_interaction = Instant::now();
    }
    if input.source == LIBRARY_LIST_ID {
        state.last_interaction = Instant::now();
    }
    if input.source == TZ_MINUS_ID || input.source == TZ_PLUS_ID {
        let delta = if input.source == TZ_PLUS_ID { 30 } else { -30 };
        let new_tz = (hw.utc_offset_minutes() + delta).clamp(-720, 840);
        hw.set_utc_offset_minutes(new_tz);
        hw.save_settings();
        // Update the label in the settings dialog.
        if let Some(v) = state.scene.get_view_mut(&TZ_LABEL_ID) {
            v.title = format_tz_label(new_tz);
            info!("marking dirty for the timezone label");
            state.scene.mark_dirty_view(&TZ_LABEL_ID);
        }
        // Immediately reflect the new offset in the clock.
        let unix_secs = hw.current_time_secs();
        if unix_secs > 0 {
            update_label(state, &TIME_LABEL_ID, format_time_local(unix_secs, new_tz));
        }
    }
}

fn handle_backlight_action(cmd: &String, hw: &mut dyn HardwareAccess) {
    hw.set_backlight_level(BacklightLevel::from_cmd(cmd.as_str()));
    hw.save_settings();
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
const LIGHT_SLEEP_AFTER_SECS: u64 = 60*5;
#[cfg(feature = "esp")]
const DEEP_SLEEP_AFTER_SECS: u64 = 3600;

/// Wraps the Gray4 e-paper display and presents an Rgb565 DrawTarget for iris-ui.
/// Converts Rgb565 luminance to 4-bit gray and applies orientation rotation so
/// the logical coordinate space matches what the user sees.
#[cfg(feature = "esp")]
struct Rgb565ToGray4<'a, I> {
    display: Display<'a, I>,
    orientation: Orientation,
}

#[cfg(feature = "esp")]
impl<'a, I: embedded_hal::i2c::I2c> Rgb565ToGray4<'a, I> {
    fn new(display: Display<'a, I>, orientation: Orientation) -> Self {
        Self {
            display,
            orientation,
        }
    }
    fn flush(&mut self) {
        self.display.flush(DrawMode::BlackOnWhite).unwrap();
    }

    fn flush_with_mode(&mut self, mode: DrawMode) {
        self.display.flush(mode).unwrap();
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
impl<'a, I: embedded_hal::i2c::I2c> embedded_graphics::draw_target::DrawTarget for Rgb565ToGray4<'a, I> {
    type Color = Rgb565;
    type Error = ();

    fn draw_iter<Iter>(&mut self, pixels: Iter) -> Result<(), Self::Error>
    where
        Iter: IntoIterator<Item = embedded_graphics::Pixel<Self::Color>>,
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
impl<'a, I: embedded_hal::i2c::I2c> embedded_graphics::geometry::OriginDimensions for Rgb565ToGray4<'a, I> {
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
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, signal::Signal};
#[cfg(feature = "esp")]
use embassy_time::{with_timeout, Duration, Instant, Timer as EmbassyTimer};
use embedded_graphics_core::geometry::Size;
use ereader::appstate::{book_from_data, cfg_from_scene, AppState};
use ereader::bookview::{draw_book_content, BookState, CONTENT_ID};
use ereader::h_spacer::make_h_spacer;
#[cfg(feature = "esp")]
use ereader::hardware::{load_tz_offset, rtc_store_read};
use ereader::{truncating_label};
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
    system::{Cpu, Stack},
    time::Rate,
    timer::timg::TimerGroup,
};
#[cfg(feature = "esp")]
use esp_radio::wifi::{sta::StationConfig, Config, ControllerConfig, Interface};
use iris_ui::input::{InputEvent, InputResult, OutputAction};
use iris_ui::panel::{make_panel, PanelState};
use iris_ui::view::Align::Start;
use iris_ui::view::Flex::Grow;
use log::info;
#[cfg(feature = "esp")]
use static_cell::StaticCell;
use Flex::{Fixed, Shrink};
use ereader::fast_paging::{FAST_SCROLL_LABEL_ID, FAST_SCROLL_PANEL_ID};
use ereader::native_screen::NativeScreen;

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
const ENABLE_WIFI_NTP: bool = true;
// Set to false to run `ui_task` on core 0 alongside everything else instead
// of starting core 1 — useful for isolating whether a bug is related to the
// dual-core split. See the flash-write hazard documented on
// `wait_for_flash_write` in src/hardware.rs; that code path stays safe
// either way, since a single-core cooperative executor never has two tasks
// running at once.
#[cfg(feature = "esp")]
const USE_SECOND_CORE: bool = false;

#[cfg(feature = "esp")]
const NTP_ADDR: [u8; 4] = [216, 239, 35, 0]; // time.google.com
#[cfg(feature = "esp")]
const NTP_UNIX_OFFSET: u64 = 2_208_988_800; // NTP epoch → Unix epoch

#[cfg(feature = "esp")]
type I2cBus = critical_section::Mutex<
    core::cell::RefCell<esp_hal::i2c::master::I2c<'static, esp_hal::Blocking>>,
>;
#[cfg(feature = "esp")]
type SharedI2c = embedded_hal_bus::i2c::CriticalSectionDevice<
    'static,
    esp_hal::i2c::master::I2c<'static, esp_hal::Blocking>,
>;

#[cfg(feature = "esp")]
static BATTERY_RESULT: Signal<CriticalSectionRawMutex, ereader::hardware::BatteryInfo> =
    Signal::new();

#[cfg(feature = "esp")]
static TIME_TICK: Signal<CriticalSectionRawMutex, ()> = Signal::new();

#[cfg(feature = "esp")]
static WIFI_SYNC_REQUEST: Signal<CriticalSectionRawMutex, ()> = Signal::new();
#[cfg(feature = "esp")]
static WIFI_SYNC_RESULT: Signal<CriticalSectionRawMutex, Option<u64>> = Signal::new();

#[cfg(feature = "esp")]
static BOOK_LOAD_REQUEST: Signal<CriticalSectionRawMutex, alloc::string::String> = Signal::new();
#[cfg(feature = "esp")]
static BOOK_LOAD_PROGRESS: Signal<CriticalSectionRawMutex, u8> = Signal::new();
#[cfg(feature = "esp")]
static BOOK_LOAD_RESULT: Signal<
    CriticalSectionRawMutex,
    Option<alloc::vec::Vec<u8>>,
> = Signal::new();

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

#[cfg(feature = "esp")]
#[embassy_executor::task]
async fn time_task() {
    loop {
        EmbassyTimer::after(Duration::from_secs(10)).await;
        TIME_TICK.signal(());
    }
}

#[cfg(feature = "esp")]
#[embassy_executor::task]
async fn battery_task(mut i2c: SharedI2c) {
    loop {
        EmbassyTimer::after(Duration::from_secs(60*5)).await;
        let voltage_mv = bq27220_read_u16(&mut i2c, 0x08) as u32;
        let current_ma = bq27220_read_i16(&mut i2c, 0x14);
        let percent = bq27220_read_u16(&mut i2c, 0x1E).min(100) as u8;
        BATTERY_RESULT.signal(ereader::hardware::BatteryInfo {
            voltage_mv,
            percent,
            is_charging: current_ma > 0,
        });
        info!("triggered battery refresh");
    }
}

#[cfg(feature = "esp")]
#[embassy_executor::task]
async fn book_load_task() {
    loop {
        let filename = BOOK_LOAD_REQUEST.wait().await;
        BOOK_LOAD_PROGRESS.signal(5);
        EmbassyTimer::after(Duration::from_millis(100)).await;

        let data = ereader::hardware::sd_read_file(&filename);

        let pct = if data.is_some() { 70 } else { 0 };
        BOOK_LOAD_PROGRESS.signal(pct);
        EmbassyTimer::after(Duration::from_millis(400)).await;

        BOOK_LOAD_RESULT.signal(data);
    }
}

#[cfg(feature = "esp")]
fn bq27220_read_u16<I: embedded_hal::i2c::I2c>(i2c: &mut I, reg: u8) -> u16 {
    let mut buf = [0u8; 2];
    let _ = i2c.write_read(0x55, &[reg], &mut buf);
    u16::from_le_bytes(buf)
}

#[cfg(feature = "esp")]
fn bq27220_read_i16<I: embedded_hal::i2c::I2c>(i2c: &mut I, reg: u8) -> i16 {
    bq27220_read_u16(i2c, reg) as i16
}

#[cfg(feature = "esp")]
#[embassy_executor::task]
async fn wifi_task(
    mut controller: esp_radio::wifi::WifiController<'static>,
    stack: embassy_net::Stack<'static>,
    skip_initial: bool,
) {
    if !skip_initial {
        do_ntp_sync(&mut controller, stack).await;
    }
    loop {
        WIFI_SYNC_REQUEST.wait().await;
        do_ntp_sync(&mut controller, stack).await;
    }
}

#[cfg(feature = "esp")]
async fn do_ntp_sync(
    controller: &mut esp_radio::wifi::WifiController<'static>,
    stack: embassy_net::Stack<'static>,
) {
    info!("NTP: connecting to '{}' ...", SSID);
    let result = with_timeout(Duration::from_secs(20), async {
        if let Err(e) = controller.connect_async().await {
            log::warn!("NTP: wifi connect failed: {:?}", e);
            return None;
        }
        info!("NTP: wifi connected, waiting for DHCP...");
        stack.wait_config_up().await;
        info!("NTP: DHCP obtained, querying time.google.com...");
        query_ntp(stack).await
    })
    .await;
    let unix_opt = result.ok().flatten();
    WIFI_SYNC_RESULT.signal(unix_opt);
    controller.disconnect_async().await.ok();
    info!("NTP: wifi disconnected");
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

/// Persisted/restored settings handed off to `ui_task` on core 1.
#[cfg(feature = "esp")]
struct BootSettings {
    font_idx: usize,
    bl_idx: usize,
    ori_idx: usize,
    saved_chapter: usize,
    saved_anchor: usize,
    tz_offset_minutes: i32,
}

#[cfg(feature = "esp")]
#[esp_rtos::main]
async fn main(spawner: Spawner) -> ! {

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
    {
        use esp_alloc::MemoryCapability;
        let psram_free = esp_alloc::HEAP.free_caps(MemoryCapability::External.into());
        let sram_free = esp_alloc::HEAP.free_caps(MemoryCapability::Internal.into());
        info!("heap init: psram_free={} sram_free={}", psram_free, sram_free);
    }

    // Must run before any EmbassyTimer use and before esp_radio::wifi::new.
    let timg0 = TimerGroup::new(peripherals.TIMG0);
    let sw_int = SoftwareInterruptControl::new(peripherals.SW_INTERRUPT);
    esp_rtos::start(timg0.timer0, sw_int.software_interrupt0);

    let rtc = Rtc::new(peripherals.LPWR);

    // Build a shared I2C bus so the battery gauge can be read from a background task.
    let i2c_raw = esp_hal::i2c::master::I2c::new(
        peripherals.I2C0,
        esp_hal::i2c::master::Config::default(),
    )
    .expect("I2C init")
    .with_sda(peripherals.GPIO39)
    .with_scl(peripherals.GPIO40);
    static I2C_BUS: StaticCell<I2cBus> = StaticCell::new();
    let i2c_bus: &'static I2cBus =
        I2C_BUS.init(critical_section::Mutex::new(core::cell::RefCell::new(i2c_raw)));
    let display_i2c = embedded_hal_bus::i2c::CriticalSectionDevice::new(i2c_bus);
    let battery_i2c = embedded_hal_bus::i2c::CriticalSectionDevice::new(i2c_bus);
    spawner.spawn(battery_task(battery_i2c).expect("battery_task spawn"));
    spawner.spawn(time_task().expect("time_task spawn"));
    spawner.spawn(book_load_task().expect("book_load_task spawn"));
    spawner.spawn(flash_write_task().expect("flash_write_task spawn"));

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
        info!(
            "woke from deep sleep: ch={} anchor={} font={} bl={} ori={}",
            chapter,
            anchor,
            font,
            bl,
            ori
        );
        (font, bl, ori, chapter, anchor)
    } else {
        let (font, bl, ori, _) = load_settings();
        let (chapter, anchor) = load_cold_boot_position();
        (font, bl, ori, chapter, anchor)
    };
    // TZ offset is always read from flash (it is not packed into RTC registers).
    let tz_offset_minutes = load_tz_offset();

    // Capture seed before rtc is moved into ui_task.
    let seed = rtc.current_time_us();

    // ── Core 1: display + touch + UI event loop ─────────────────────────────
    // Runs on the ESP32-S3's second core, in its own embassy executor, so a
    // blocking e-paper flush or SD card read never stalls the WiFi/NTP/
    // battery/time tasks below, which stay on core 0's executor.
    let pin_cfg = ereader::pin_config!(peripherals);
    let dma_ch0 = peripherals.DMA_CH0;
    let lcd_cam = peripherals.LCD_CAM;
    let rmt_periph = peripherals.RMT;
    let ledc_periph = peripherals.LEDC;
    let gpio11 = peripherals.GPIO11;
    let gpio0 = peripherals.GPIO0;
    let gpio38 = peripherals.GPIO38;
    let cpu_ctrl = peripherals.CPU_CTRL;
    let sw_int1 = sw_int.software_interrupt1;
    let boot = BootSettings {
        font_idx,
        bl_idx,
        ori_idx,
        saved_chapter,
        saved_anchor,
        tz_offset_minutes,
    };

    static CORE1_STACK: StaticCell<Stack<16384>> = StaticCell::new();
    let core1_stack = CORE1_STACK.init(Stack::new());
    static CORE1_EXECUTOR: StaticCell<esp_rtos::embassy::Executor> = StaticCell::new();

    // Constructed here (before `stack`/WiFi exist) so both possible `ui_task`
    // spawn sites below and the debug server spawned later can all share it.

    if USE_SECOND_CORE {
        esp_rtos::start_second_core(cpu_ctrl, sw_int1, core1_stack, move || {
            let executor = CORE1_EXECUTOR.init(esp_rtos::embassy::Executor::new());
            executor.run(|spawner| {
                spawner.spawn(
                    ui_task(
                        pin_cfg,
                        dma_ch0,
                        lcd_cam,
                        rmt_periph,
                        display_i2c,
                        ledc_periph,
                        gpio11,
                        gpio0,
                        gpio38,
                        rtc,
                        boot,
                    )
                    .expect("ui_task spawn"),
                );
            });
        });
    } else {
        info!("USE_SECOND_CORE = false — running ui_task on core 0");
        spawner.spawn(
            ui_task(
                pin_cfg,
                dma_ch0,
                lcd_cam,
                rmt_periph,
                display_i2c,
                ledc_periph,
                gpio11,
                gpio0,
                gpio38,
                rtc,
                boot,
            )
            .expect("ui_task spawn"),
        );
    }

    // ── WiFi + NTP time sync (background task, core 0) ──────────────────────
    // wifi_task connects, syncs NTP, then disconnects. It stays alive to handle
    // future sync requests (e.g. from the sync_time button) via WIFI_SYNC_REQUEST.
    if ENABLE_WIFI_NTP {
        info!("connecting to WIFI_SSID {} WIFI_PASS {}", SSID, PASSWORD);
        let station_config = Config::Station(
            StationConfig::default()
                .with_ssid(SSID)
                .with_password(PASSWORD.into()),
        );
        let (controller, interfaces) = esp_radio::wifi::new(
            peripherals.WIFI,
            ControllerConfig::default().with_initial_config(station_config),
        )
        .expect("wifi init");
        let stack_resources = mk_static!(StackResources<3>, StackResources::<3>::new());
        let (stack, runner) = embassy_net::new(
            interfaces.station,
            embassy_net::Config::dhcpv4(Default::default()),
            stack_resources,
            seed,
        );
        spawner.spawn(net_task(runner).expect("net_task spawn"));
        info!("spawned net_task");
        spawner.spawn(wifi_task(controller, stack, is_sleep_wakeup).expect("wifi_task spawn"));
        info!("spawned wifi_task");
    } else {
        info!("WiFi/NTP disabled (ENABLE_WIFI_NTP = false)");
    }

    // Core 0 has nothing left to do but keep the background tasks (battery,
    // time, book loading, WiFi/NTP) alive; the UI runs entirely on core 1.
    loop {
        EmbassyTimer::after(Duration::from_secs(3600)).await;
    }
}

#[cfg(feature = "esp")]
pub struct EspNativeScreen<'a> {
    pub bridge: Rgb565ToGray4<'a, SharedI2c>,
}
#[cfg(feature = "esp")]
impl EspNativeScreen<'_> {

}
#[cfg(feature = "esp")]
impl NativeScreen for EspNativeScreen<'_> {
    fn resize(&mut self, _size: Size) {
        todo!()
    }

    fn deep_clean(&mut self, state: &mut AppState) {
        self.bridge.display.deep_clean(3).unwrap();
        state.partial_refresh_count = 0;
        info!("marked dirty all for deep clean");
        state.scene.mark_dirty_all();
    }

    fn deep_sleep(&mut self, state: &mut AppState) {
        let sleep_dirty = state.scene.dirty_rect.clone();
        // Clear away the previous screen (e.g. the settings dialog) first —
        // drawing straight over it without a clearing pass leaves ghosting
        // and can leave the sleep message only partially visible.
        self.bridge.clearing_flush_region(&sleep_dirty);
        {
            let mut ctx = EmbeddedDrawingContext::new(&mut self.bridge);
            ctx.clip = sleep_dirty;
            layout_scene(&mut state.scene, &state.theme);
            draw_scene(&mut state.scene, &mut ctx, &state.theme);
        }
        self.bridge.flush();
        self.bridge.display.power_off();
    }

    fn refresh(&mut self, state: &mut AppState) {
        let dirty = state.scene.dirty_rect.clone();
        self.bridge.clearing_flush_region(&dirty);
        {
            let mut ctx = EmbeddedDrawingContext::new(&mut self.bridge);
            ctx.clip = dirty.clone();
            layout_scene(&mut state.scene, &state.theme);
            draw_scene(&mut state.scene, &mut ctx, &state.theme);
        }
        self.bridge.flush_region(&dirty, DrawMode::BlackOnWhite);
        // let dirty = state.scene.dirty_rect.clone();
        // if !dirty.is_empty() {
        //     info!("marking manually drawing and flushing for the book load progress");
        //     {
        //         let mut ctx = EmbeddedDrawingContext::new(&mut bridge);
        //         ctx.clip = dirty.clone();
        //         // Skip layout_scene — dialog geometry is stable between progress
        //         // updates (was already laid out during the initial full-screen render).
        //         draw_scene(&mut state.scene, &mut ctx, &state.theme);
        //     }
        //     bridge.flush_region(&dirty, DrawMode::Fast);
        //     state.partial_refresh_count += 1;
        // }
    }
}


#[cfg(feature = "esp")]
#[embassy_executor::task]
async fn ui_task(
    pin_cfg: ereader::driver::PinConfig<'static>,
    dma_ch0: esp_hal::peripherals::DMA_CH0<'static>,
    lcd_cam: esp_hal::peripherals::LCD_CAM<'static>,
    rmt_periph: esp_hal::peripherals::RMT<'static>,
    display_i2c: SharedI2c,
    ledc_periph: esp_hal::peripherals::LEDC<'static>,
    gpio11: esp_hal::peripherals::GPIO11<'static>,
    gpio0: esp_hal::peripherals::GPIO0<'static>,
    gpio38: esp_hal::peripherals::GPIO38<'static>,
    rtc: Rtc<'static>,
    boot: BootSettings,
) {
    use esp_println::println;
    use ereader::fast_paging::FastPaging;

    let mut display = Display::new(pin_cfg, dma_ch0, lcd_cam, rmt_periph, display_i2c)
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

    let mut ledc = Ledc::new(ledc_periph);
    ledc.set_global_slow_clock(LSGlobalClkSource::APBClk);
    let mut lstimer0 = ledc.timer::<LowSpeed>(timer::Number::Timer0);
    lstimer0
        .configure(timer::config::Config {
            duty: timer::config::Duty::Duty8Bit,
            clock_source: timer::LSClockSource::APBClk,
            frequency: Rate::from_khz(1),
        })
        .unwrap();
    let mut bl_ch = ledc.channel(channel::Number::Channel0, gpio11);
    bl_ch
        .configure(channel::config::Config {
            timer: &lstimer0,
            duty_pct: 100,
            drive_mode: esp_hal::gpio::DriveMode::PushPull,
        })
        .unwrap();

    let BootSettings {
        font_idx,
        bl_idx,
        ori_idx,
        mut saved_chapter,
        mut saved_anchor,
        tz_offset_minutes,
    } = boot;

    // Physical buttons: BOOT (GPIO0, active-low) = prev page; GPIO38 = next page.
    let btn_prev = Input::new(gpio0, InputConfig::default().with_pull(Pull::Up));
    let btn_next = Input::new(gpio38, InputConfig::default().with_pull(Pull::Up));

    let mut hw = EspHardware::new(
        bl_ch,
        rtc,
        btn_prev,
        btn_next,
        FontSize::from_index(font_idx),
        BacklightLevel::from_index(bl_idx),
        Orientation::from_index(ori_idx),
        tz_offset_minutes,
    );
    let mut native_screen = EspNativeScreen {
        bridge: Rgb565ToGray4::new(display, hw.orientation())
    };
    let handlers = vec![handle_click as Callback<Rgb565>];
    let mut was_touching = false;
    let mut last_touch_point: Option<GPoint> = None;

    // Counts consecutive partial refreshes so we can force a full ghost-clear
    // periodically.  E-paper capacitive field coupling from repeated partial
    // waveform passes slowly darkens white areas adjacent to the dirty rect;
    // a full refresh resets all pixels to a clean state.
    let mut state: AppState = init_app_state(&hw);

    /// Sync the settings dialog toggle groups to reflect the actual loaded settings.
    /// make_scene() hardcodes default selections; call this after loading persisted values.
    fn sync_settings_ui(scene: &mut Scene<Rgb565>, font_idx: usize, bl_idx: usize, ori_idx: usize) {
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
    // Update the TZ label to show the persisted offset.
    if let Some(v) = state.scene.get_view_mut(&TZ_LABEL_ID) {
        v.title = format_tz_label(tz_offset_minutes);
    }

    // On any boot (sleep wakeup or hard reset), reopen the last-read SD card book.
    // The filename is saved to NVS when a book is opened and again before deep sleep.
    if let Some(last_file) = load_last_filename() {
        show_loading_dialog(&mut state.scene, &last_file);
        {
            native_screen.refresh(&mut state);
        }
        if let Some(data) = hw.load_book_file(&last_file) {
            state.book = book_from_data(&last_file, data);
            if let Some((ch, anch)) = hw.load_bookmark(&last_file) {
                saved_chapter = ch;
                saved_anchor = anch;
            }
            state.current_filename = last_file;
        }
        state.scene.hide_view(&LOADING_DIALOG_ID);
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
    let mut pending_load: Option<String> = None;
    // Highest flash-write id we've confirmed landed on flash. Advanced right
    // before every display flush below — see the module comment in
    // src/hardware.rs on why a write racing a render corrupts the flash
    // cache for whichever core isn't doing the write.
    let mut last_confirmed_flash_write: u32 = 0;
    const PARTIAL_REFRESH_FULL_INTERVAL: u32 = 8;
    // Full-quality (15-frame) refresh every N full-screen page turns.
    // In between, use the 4-frame fast waveform.  Increase to reduce flicker;
    // decrease if ghosting accumulates too quickly.
    const FULL_QUALITY_INTERVAL: u32 = 5;

    let mut fast = FastPaging::default();

    #[derive(Clone, Copy)]
    enum Btn { Prev, Next, Face }
    impl Btn {
        fn name(self) -> &'static str {
            match self { Btn::Prev => "BOOT", Btn::Next => "SIDE", Btn::Face => "FACE" }
        }
        fn is_forward(self) -> bool { !matches!(self, Btn::Prev) }
    }

    loop {
        // Apply background battery reading if a new one has arrived.
        if let Some(info) = BATTERY_RESULT.try_take() {
            hw.update_battery(info.voltage_mv, info.percent, info.is_charging);
            update_battery_labels(&mut state.scene, &hw);
        }

        // Refresh the time label from the RTC every 10 s (driven by time_task).
        if TIME_TICK.try_take().is_some() {
            let unix_secs = hw.current_time_secs();
            if unix_secs > 0 {
                let time_str = format_time_local(unix_secs, hw.utc_offset_minutes());
                update_label(&mut state,&TIME_LABEL_ID, time_str);
            }
        }

        // Apply NTP sync result if wifi_task delivered one.
        if let Some(unix_opt) = WIFI_SYNC_RESULT.try_take() {
            match unix_opt {
                Some(unix_secs) => {
                    hw.set_current_time_secs(unix_secs);
                    let time_str = format_time_local(unix_secs, hw.utc_offset_minutes());
                    info!("NTP synced: {}", time_str);
                    update_label(&mut state,&TIME_LABEL_ID, time_str);
                    info!("marked ntp time as dirty");
                }
                None => log::warn!("NTP: sync failed (no response)"),
            }
        }

        // Update progress bar while book_load_task is running, and flush
        // the updated dialog to the display immediately so the bar is visible
        // before the next blocking SD read or before the result arrives.
        if let Some(pct) = BOOK_LOAD_PROGRESS.try_take() {
            set_loading_progress(&mut state.scene, pct);
            native_screen.refresh(&mut state);
        }

        // Finalize the load when book_load_task delivers the raw bytes.
        if let Some(data) = BOOK_LOAD_RESULT.try_take() {
            if let Some(filename) = pending_load.take() {
                match data {
                    None => {
                        state.scene.hide_view(&LOADING_DIALOG_ID);
                        show_error_dialog(&mut state.scene, &filename);
                    }
                    Some(bytes) => {
                        set_loading_progress(&mut state.scene, 90);
                        let book = book_from_data(&filename, bytes);
                        state.cfg = cfg_from_scene(
                            &mut state.scene,
                            &state.theme,
                            &state.fonts,
                            hw.font_size(),
                        );
                        let new_session = match hw.load_bookmark(&filename) {
                            Some((ch, anchor)) => {
                                BookSession::restore(book.as_ref(), &state.cfg, ch, anchor)
                                    .or_else(|_| BookSession::new(book.as_ref(), &state.cfg))
                            }
                            None => BookSession::new(book.as_ref(), &state.cfg),
                        };
                        state.scene.hide_view(&LOADING_DIALOG_ID);
                        match new_session {
                            Ok(session) => {
                                hw.save_bookmark(
                                    &state.current_filename,
                                    state.session.chapter_idx,
                                    state.session.reader.anchor_byte,
                                );
                                state.current_filename = filename.clone();
                                hw.save_last_filename(&filename);
                                state.session = session;
                                state.book = book;
                                state.update_content(&hw);
                            }
                            Err(_) => {
                                show_error_dialog(&mut state.scene, &filename);
                            }
                        }
                    }
                }
                state.last_interaction = Instant::now();
            }
        }

        // Read GT911 touch + face-button key state in one I2C transaction so we
        // don't miss a key event that read_touch would silently discard.
        let (touch_pt, face_key) = native_screen.bridge.display.read_touch_and_key(&mut gt911);

        // Physical button handling:
        //   BOOT (GPIO0)  → prev page
        //   SIDE (GPIO38) → next page
        //   FACE (GT911 key area below screen) → next page
        // Short press → single page turn. Hold > 1 s → fast-scroll mode.
        // The book content only re-renders on release.
        let btn_pressed: Option<Btn> = if hw.button_prev_pressed() {
            Some(Btn::Prev)
        } else if hw.button_next_pressed() {
            Some(Btn::Next)
        } else if face_key {
            Some(Btn::Face)
        } else {
            None
        };
        if let Some(btn) = btn_pressed {
            info!("{} button pressed{}", btn.name(), if just_woke { " (wake)" } else { "" });
            if !just_woke {
                if btn.is_forward() { fast.start_forward(); } else { fast.start_backward(); }
            }
            loop {
                let still_held = match btn {
                    Btn::Prev => hw.button_prev_pressed(),
                    Btn::Next => hw.button_next_pressed(),
                    Btn::Face => native_screen.bridge.display.gt911_key_pressed(&mut gt911),
                };
                if !still_held {
                    break;
                }

                fast.handle_update_label(&mut state);

                // During fast paging we only ever repaint the small counter panel.
                // If mark_layout_dirty() was called (first show of the panel), the
                // dirty_rect is the full screen — we don't need that.  Run layout
                // first so the panel has its centred global bounds, then rederive
                // the dirty rect from the panel alone.
                if !state.scene.dirty_rect.is_empty() {
                    info!("calling layout scene for fast paging");
                    layout_scene(&mut state.scene, &state.theme);
                    state.scene.dirty_rect = Bounds::new_empty();
                    state.scene.mark_dirty_view(&FAST_SCROLL_PANEL_ID);
                    let panel_rect = state.scene.dirty_rect.clone();

                    info!("flushing for fast paging");
                    native_screen.bridge.flush_region(&panel_rect, DrawMode::FastClear);
                    {
                        let mut ctx = EmbeddedDrawingContext::new(&mut native_screen.bridge);
                        ctx.clip = panel_rect;
                        draw_scene(&mut state.scene, &mut ctx, &state.theme);
                    }
                    native_screen.bridge.flush_region(&panel_rect, DrawMode::Fast);
                }

                EmbassyTimer::after(Duration::from_millis(10)).await;
            }
            info!("{} button released", btn.name());

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

        // Drain any flash write enqueued above (or by handle_click_action
        // further below, in a previous iteration) before touching the
        // display: a write landing on core 0 mid-render corrupts the flash
        // cache for core 1 too. See src/hardware.rs's module comment.
        let pending_flash_write = ereader::hardware::latest_flash_write_id();
        if pending_flash_write > last_confirmed_flash_write {
            ereader::hardware::wait_for_flash_write(pending_flash_write).await;
            last_confirmed_flash_write = pending_flash_write;
        }

        let dirty_rect = state.scene.dirty_rect.clone();
        let was_dirty = !dirty_rect.is_empty();
        let (scene_w, scene_h) = hw.orientation().logical_size();
        let needs_full_refresh = dirty_rect.size.w >= scene_w && dirty_rect.size.h >= scene_h;

        if was_dirty {
            info!("clip rect {}", state.scene.dirty_rect);
            // Ghost-clear pass: drives dark pixels to white so the draw pass can
            // correctly lighten pixels that changed from dark to light.
            // Periodic full refresh: accumulating field coupling slowly darkens
            // white areas; every PARTIAL_REFRESH_FULL_INTERVAL partials we force a full.
            // Fast waveforms (4 frames) are used for page turns; every
            // FULL_QUALITY_INTERVAL full-screen turns we fall back to 15-frame quality.
            let force_full =
                !needs_full_refresh && state.partial_refresh_count >= PARTIAL_REFRESH_FULL_INTERVAL;
            let use_fast = needs_full_refresh
                && !force_full
                && state.full_quality_count < FULL_QUALITY_INTERVAL;
            let (clear_mode, draw_mode) = if use_fast {
                (DrawMode::FastClear, DrawMode::Fast)
            } else {
                (DrawMode::WhiteOnBlack, DrawMode::BlackOnWhite)
            };
            if needs_full_refresh || force_full {
                native_screen.bridge.display.fill(0x0F).unwrap();
                native_screen.bridge.display.flush(clear_mode).unwrap();
                state.partial_refresh_count = 0;
                if needs_full_refresh {
                    if use_fast {
                        state.full_quality_count += 1;
                    } else {
                        state.full_quality_count = 0;
                    }
                }
            } else {
                native_screen.bridge.clearing_flush_region(&dirty_rect);
                state.partial_refresh_count += 1;
            }
            {
                let mut ctx = EmbeddedDrawingContext::new(&mut native_screen.bridge);
                ctx.clip = if force_full {
                    Bounds::new(0, 0, scene_w, scene_h)
                } else {
                    dirty_rect.clone()
                };
                layout_scene(&mut state.scene, &state.theme);
                draw_scene(&mut state.scene, &mut ctx, &state.theme);
            }
            if needs_full_refresh || force_full {
                native_screen.bridge.flush_with_mode(draw_mode);
            } else {
                native_screen.bridge.flush_region(&dirty_rect, DrawMode::BlackOnWhite);
            }
        }

        if let Some((tx, ty)) = touch_pt {
            let (lx, ly) = hw.orientation().phys_to_logical(tx, ty);
            let pt = GPoint::new(lx, ly);
            if !was_touching {
                pointer_down_at(&mut state.scene, &handlers, pt.clone());
            }
            last_touch_point = Some(pt);
            was_touching = true;
        } else if was_touching {
            was_touching = false;
            if let Some(pt) = last_touch_point.take() {
                if let Some(input) = pointer_up_at(&mut state.scene, &handlers, pt) {
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
                            native_screen.bridge.orientation = hw.orientation();
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
                    // Refresh battery from BQ27220 fuel gauge (I2C 0x55).
                    // Voltage at reg 0x08, AverageCurrent at 0x14, StateOfCharge at 0x1E.
                    if input.source == BATTERY_BUTTON_ID {
                        let voltage_mv = native_screen.bridge.display.i2c_read_u16(0x55, 0x08) as u32;
                        let current_ma = native_screen.bridge.display.i2c_read_i16(0x55, 0x14);
                        let percent = native_screen.bridge.display.i2c_read_u16(0x55, 0x1E).min(100) as u8;
                        let is_charging = current_ma > 0;
                        hw.update_battery(voltage_mv, percent, is_charging);
                    }
                    handle_click_action(&mut hw, &input, &mut state);
                    if input.source == ViewId::new("sync_time") {
                        if ENABLE_WIFI_NTP {
                            info!("sync_time pressed — requesting background NTP sync");
                            WIFI_SYNC_REQUEST.signal(());
                        } else {
                            info!("sync_time pressed but WiFi is disabled");
                        }
                    } else if input.source == DEEP_CLEAN_ID {
                        info!("deep clean started");
                        native_screen.deep_clean(&mut state);
                    } else if input.source == DEEP_SLEEP_BUTTON_ID {
                        info!("deep sleep button pressed — entering deep sleep");
                        state.scene.hide_view(&SETTINGS_DIALOG_ID);
                        state.scene.show_view(&SLEEP_DIALOG_ID);
                        info!("marked dirty all for deep sleep button");
                        state.scene.mark_dirty_all();
                        // Render the sleep message before powering off.
                        native_screen.deep_sleep(&mut state);
                        // Persist filename and position so wakeup can reopen the same book.
                        save_before_sleep(
                            &state.current_filename,
                            state.session.chapter_idx,
                            state.session.reader.anchor_byte,
                        )
                        .await;
                        // Never returns on ESP.
                        hw.enter_deep_sleep(
                            state.session.chapter_idx,
                            state.session.reader.anchor_byte,
                        );
                    } else if input.source == LIBRARY_READ_BUTTON_ID {
                        let filename = state
                            .scene
                            .get_view_mut(&LIBRARY_LIST_ID)
                            .and_then(|v| v.get_state::<ListState>())
                            .and_then(|s| s.items.get(s.selected).cloned());
                        if let Some(filename) = filename {
                            state.scene.hide_view(&LIBRARY_DIALOG_ID);
                            show_loading_dialog(&mut state.scene, &filename);
                            // Flush the loading dialog (with 0% bar) to e-paper immediately.
                            {
                                let dirty = state.scene.dirty_rect.clone();
                                native_screen.bridge.clearing_flush_region(&dirty);
                                {
                                    let mut ctx = EmbeddedDrawingContext::new(&mut native_screen.bridge);
                                    ctx.clip = dirty.clone();
                                    layout_scene(&mut state.scene, &state.theme);
                                    draw_scene(&mut state.scene, &mut ctx, &state.theme);
                                }
                                native_screen.bridge.flush_region(&dirty, DrawMode::BlackOnWhite);
                                state.partial_refresh_count += 1;
                            }
                            // Drop the old book now so its heap is free before the
                            // background task allocates the new epub (which can be
                            // larger than the remaining free heap if the old book
                            // is still live).
                            state.book = Box::new(TxtBook::from_vec(alloc::vec![]));
                            // Hand off to book_load_task; result arrives via BOOK_LOAD_RESULT.
                            pending_load = Some(filename.clone());
                            BOOK_LOAD_REQUEST.signal(filename);
                        }
                        state.last_interaction = Instant::now();
                    } else {
                        state.last_interaction = Instant::now();
                    }
                }
            }
        }

        // Two-tier inactivity sleep: light sleep at 60 s, deep sleep at 60 min.
        let elapsed_secs = state.last_interaction.elapsed().as_secs();
        if elapsed_secs >= DEEP_SLEEP_AFTER_SECS {
            info!("inactivity timeout — entering deep sleep");
            state.scene.show_view(&SLEEP_DIALOG_ID);
            info!("marking dirty all for showing the sleep dialog");
            state.scene.mark_dirty_all();
            // Render the sleep message before powering off.
            let sleep_dirty = state.scene.dirty_rect.clone();
            // Clear away the previous screen first — see comment on the
            // button-triggered deep sleep path above.
            native_screen.bridge.clearing_flush_region(&sleep_dirty);
            {
                let mut ctx = EmbeddedDrawingContext::new(&mut native_screen.bridge);
                ctx.clip = sleep_dirty;
                layout_scene(&mut state.scene, &state.theme);
                draw_scene(&mut state.scene, &mut ctx, &state.theme);
            }
            native_screen.bridge.flush();
            native_screen.bridge.display.power_off();
            // Persist filename and position so wakeup can reopen the same book.
            // Unlike the fire-and-forget saves elsewhere, this one must be
            // confirmed written before we power off — there's no next tick
            // for core 0 to catch up on.
            save_before_sleep(
                &state.current_filename,
                state.session.chapter_idx,
                state.session.reader.anchor_byte,
            )
            .await;
            // On ESP: saves RTC state and enters deep sleep (never returns).
            // On simulator enter_deep_sleep is a no-op; reset the timer so we
            // don't loop immediately back into the sleep check.
            hw.enter_deep_sleep(state.session.chapter_idx, state.session.reader.anchor_byte);
            state.last_interaction = Instant::now();
            state.update_content(&hw);
        } else if elapsed_secs >= LIGHT_SLEEP_AFTER_SECS {
            info!("inactivity timeout — entering light sleep");
            // Draw a grid of 30×30 black squares with white 5 px gaps on the physical display.
            // fill(0x0F) sets the entire framebuffer to white, making the gaps white by default.
            {
                const CELL: u16 = 30;
                const GAP: u16 = 5;
                const STRIDE: u16 = CELL + GAP;
                native_screen.bridge.display.fill(0x0F).unwrap(); // white — gaps inherit this
                let pw = ereader::driver::display::DISPLAY_WIDTH;
                let ph = ereader::driver::display::DISPLAY_HEIGHT;
                let cols = (pw + GAP) / STRIDE;
                let rows = (ph + GAP) / STRIDE;
                for row in 0..rows {
                    for col in 0..cols {
                        let x = col * STRIDE;
                        let y = row * STRIDE;
                        native_screen.bridge.display.fill_region(
                            Rectangle { x, y, width: CELL, height: CELL },
                            0x00, // black square
                        ).unwrap();
                    }
                }
                native_screen.bridge.display.flush(DrawMode::BlackOnWhite).unwrap();
            }
            // Backlight is turned off inside enter_light_sleep and restored on return.
            hw.enter_light_sleep();
            // Full redraw after waking so the grid is replaced with book content.
            info!("full redraw after waking");
            state.scene.mark_dirty_all();
            state.last_interaction = Instant::now();
            just_woke = true;
        }

        EmbassyTimer::after(Duration::from_millis(50)).await;
    }
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
            let font_px = font_px_for(font_size);
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
