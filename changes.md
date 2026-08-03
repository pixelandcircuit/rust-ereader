# Changes

## 2026-08-03 (Move book content into iris-ui View using BookState)

Book text is now rendered entirely inside the scene tree through a plain iris-ui `View` with a custom draw function, eliminating the separate post-scene TTF render pass.

- `BookState { text: String, font_px: f32 }` — stores the current page text and font size; lives in `view.state` as `Box<dyn Any>`.
- `draw_book_content(e: &mut DrawEvent)` — bare `fn` matching `DrawFn`; reads `BookState` via `e.view.get_state::<BookState>()`, fills white, then renders TTF glyphs pixel-by-pixel via `e.ctx.put_pixel`.
- The content `View` in `make_scene` is now a plain `View { draw: Some(draw_book_content), state: Some(Box::new(BookState { ... })), ..Default::default() }` instead of a `make_panel`.
- `update_content` now takes a `font_px: f32` parameter and pushes the current page text and font size into `BookState` each time the page changes.
- Removed the separate TTF render loops from both the simulator and ESP main loops.
- Removed the now-unused `bounds_overlap` helper.

## 2026-08-02 (Add put_pixel to iris-ui DrawingContext)

Implemented `put_pixel(x, y, color)` on the `DrawingContext` trait and its `EmbeddedDrawingContext` concrete implementation in the local iris-ui build (`/Users/josh/RustroverProjects/rust-embedded-gui/`):

- `gfx.rs`: Added default no-op `put_pixel` to the `DrawingContext` trait so all existing implementors (including `MockDrawingContext`) keep compiling without changes.
- `device.rs`: Implemented `put_pixel` on `EmbeddedDrawingContext<T>` following the same clip / translate / scale pattern as the other draw methods:
  - scale == 1: clips to `self.clip`, applies `self.offset`, emits a single `Pixel` via `draw_iter`
  - scale > 1: routes through `ScaledDisplay` so each logical pixel becomes a scale×scale filled block on the physical display

This unblocks using TTF rasterization (which emits one gray pixel at a time via a callback) inside an iris-ui View's draw function.

## 2026-08-01 (Add unit tests for layout engine)

Added 11 `#[test]` functions to `src/layout.rs` covering:
- Basic word wrap splitting at spaces
- Long words that exceed line width placed alone without looping
- Hard newlines forcing line breaks
- Double newlines pushing paragraphs to new pages
- Multi-page layout with contiguous, gap-free page ranges
- Last page `end` equalling `text.len()`
- Edge cases: empty text, leading spaces, zero `line_height`

Tests use a fixed-width character metric (no TTF dependency) and run with:
`cargo test --lib --no-default-features --features simulator --target aarch64-apple-darwin`

## 2026-08-01 (Improve WiFi connection logging)

- Print SSID before attempting to connect: `NTP: connecting to 'MyNetwork' ...`
- Log the actual `WifiError` variant on failure instead of discarding it
- Include SSID in the timeout warning so misconfigured credentials are obvious in logs

## 2026-08-01 (WiFi + NTP time sync on cold boot)

On cold boot the device now connects to WiFi, queries `time.google.com` via NTP,
sets the RTC to the returned Unix timestamp, then disconnects. On deep-sleep wakeup
the step is skipped entirely because the RTC already holds the correct time.

- `src/hardware.rs`: added `set_current_time_secs(secs: u64)` to the `HardwareAccess`
  trait; `EspHardware` calls `rtc.set_current_time_us(secs * 1_000_000)`; `SimHardware`
  is a no-op (simulator reads system clock)
- `examples/ereader_ui.rs` (ESP): added `query_ntp` async function (48-byte NTP client
  over UDP, extracts transmit timestamp at bytes [40..44], subtracts `NTP_UNIX_OFFSET`);
  WiFi bring-up and NTP are wrapped in a 20-second `with_timeout`; on success the RTC is
  set via `hw.set_current_time_secs()` and the time label updated; WiFi is disconnected
  immediately after; removed persistent `wifi_connection` task (one-shot connect/disconnect
  is sufficient)

## 2026-08-01 (Fix word wrapping in ereader_ui renderer)

`next_ttf_line` used `text.find('\n')` to check for hard newlines before entering
the width-limited scan loop. This meant the entire text before the first newline —
potentially a whole paragraph — was returned as one line and clipped at the right
edge. Fixed by scanning character by character so newline breaks and width overflow
are checked in the same pass.

## 2026-08-01 (Fix pagination: use real TTF metrics in ereader_ui layout)

Two mismatches caused pages to appear only half-filled:
1. **Advance width**: `measure_medium` used a hardcoded 13 px/char average, but real
   Noticia Text advance at 22 px is ~9–10 px. The layout engine packed far fewer bytes
   per page than the renderer could display.
2. **Line height**: layout used a hardcoded 30 px/line for Medium, but the renderer uses
   `renderer.line_height(22.0) + 4 ≈ 26 px`. The renderer could fit ~34 lines in the
   space the layout allocated for 29, so pages rendered only ~65% full.

- `src/layout.rs`: changed `FontMetrics.measure` from `fn(&str) -> u32` (bare function
  pointer) to `Box<dyn Fn(&str) -> u32>` so a `TextRenderer` can be captured; added
  `use alloc::boxed::Box` for the ESP no_std path
- `examples/ereader_ui.rs`: rewrote `layout_cfg` to create a `TextRenderer`, derive
  `line_height_px` from `renderer.line_height(font_px) + 4` (exactly matching the
  renderer's own `line_h`), derive `space_width_px` from `renderer.char_advance(' ', font_px)`,
  and use `Box::new(move |s| renderer.measure_width(s, font_px))` for the measure
  closure; removed the approximate `measure_small/medium/large` functions; corrected
  `bar_h` to include the iris-ui topbar/bottombar padding (+16 instead of +8)
- `examples/epub_test.rs`: wrapped existing `measure` values in `Box::new(...)` to
  match the updated API

## 2026-08-01 (Add deep sleep and physical button support to ereader_ui)

- `src/hardware.rs`: added `button_prev_pressed`, `button_next_pressed`, and
  `enter_deep_sleep(chapter_idx, anchor_byte)` to the `HardwareAccess` trait
- `src/hardware.rs`: added `pub fn rtc_store_read/write(idx, val)` free functions
  (ESP-only) for RTC fast-memory access that survives deep sleep
- `src/hardware.rs` `EspHardware`: added `btn_prev: Input<'d>` and `btn_next: Input<'d>`
  fields; updated `new()` to accept them; implemented `button_prev/next_pressed` via
  `is_low()`; `enter_deep_sleep` packs font/backlight/orientation into RTC store5,
  anchor_byte into store0, chapter_idx into store6, zeros the LEDC duty, then calls
  `rtc.sleep_deep()` with GPIO0 as the `Ext0WakeupSource` (never returns on ESP)
- `src/hardware.rs` `SimHardware`: no-op `enter_deep_sleep`, always-false button stubs
- `examples/ereader_ui.rs` (ESP): detect deep-sleep wakeup via `reset_reason(Cpu::ProCpu)`;
  on wakeup restore position from RTC fast memory instead of NVS flash; create GPIO0 and
  GPIO38 `Input` handles and pass to `EspHardware::new()`; track `last_interaction:
  embassy_time::Instant`; poll buttons every loop iteration with release-debounce; reset
  `last_interaction` on any touch or button press; after `SLEEP_AFTER_SECS` (60 s)
  inactivity show "Sleeping…" footer, flush display, call `power_off()`, then
  `hw.enter_deep_sleep()`
- `examples/ereader_ui.rs` (simulator): added Left/Backspace → prev page and
  Right/Space → next page keyboard shortcuts via `SimulatorEvent::KeyDown`

## 2026-08-01 (Switch ereader_ui content rendering to Noticia Text TTF)

- `examples/ereader_ui.rs`: replaced bitmap-font content rendering with Noticia Text
  TrueType via `ereader::font::TextRenderer`
- `draw_content` now only fills the white background; TrueType text is drawn outside
  iris-ui after `draw_scene` — avoids the lack of pixel-level drawing in `DrawingContext`
- Added `render_ttf_text(text, font_px, bounds, put_pixel)` — shared word-wrap +
  rasterise loop that accepts a per-pixel closure, so the same logic drives both the
  simulator (`DrawTarget::draw_iter` with gray→Rgb565 conversion) and the ESP
  (`Display::set_pixel` with orientation mapping)
- Added `next_ttf_line` for TTF-metric word wrapping (advances use `char_advance`,
  handles hard `\n` breaks) and `bounds_overlap` to detect when the content view was
  repainted (to avoid unnecessary TTF passes on non-content dirty events)
- Added `font_px_for(FontSize) -> f32`: Small=16 px, Medium=22 px, Large=28 px
- Updated `layout_cfg` with approximate Noticia Text average advance widths and line
  heights for accurate pagination: Small (9 px / 22 px lh), Medium (13 px / 30 px lh),
  Large (16 px / 38 px lh)
- Removed `next_line`, `TextStyle` import, and content `view.title` setting from
  `update_content` (text is now read directly from `session.reader.current_text()`)

## 2026-08-01 (Wire BookSession into ereader_ui)

- `examples/ereader_ui.rs`: replaced static `BOOK_TEXT` with a real EPUB loaded
  via `include_bytes!("sherlock_holmes.epub")` and `EpubArchive`
- Added three character-width measure functions (`measure_small/medium/large`) and
  `layout_cfg(font, w, h)` helper that computes `LayoutConfig` dimensions to match
  `draw_content`'s rendering geometry (subtracts topbar/bottombar heights and padding)
- `draw_content` now renders `e.view.title` instead of static text; title is set by
  `update_content()` to the current page's text slice from `BookSession`
- Added `update_content(scene, session)` helper that pushes current-page text into
  the `content` view and updates the `chapter` / `page` footer labels
- Bottom bar: replaced hardcoded chapter/page labels with `< Prev` + `Next >` nav
  buttons plus live chapter/page count labels
- **Simulator**: `BookSession` created from the EPUB on startup; `prev_page` /
  `next_page` click handlers advance pages and cross chapter boundaries; font-size
  and orientation changes trigger `relayout()` and re-render
- **ESP**: same navigation plus NVS persistence — added `KEY_CHAPTER=13` /
  `KEY_ANCHOR=14`, `load_position()` / `save_position()` functions; on startup the
  position is restored via `BookSession::restore()`; position is saved to flash after
  every page turn

## 2026-08-01 (Add BookSession abstraction)

- `src/reader.rs`: added `BookSession` struct that combines the EPUB spine with
  the current chapter index and an inner `ReaderState`. Provides:
  - `new(epub, cfg)` — opens the EPUB and loads chapter 0
  - `restore(epub, cfg, chapter_idx, anchor_byte)` — restores a saved position
    in one layout pass (no double pagination)
  - `go_to_chapter(idx, epub, cfg)` — load an arbitrary chapter by spine index
  - `next_chapter` / `prev_chapter` — return `false` at the spine boundaries
  - `chapter_count()` / `spine()` — read the spine without re-parsing
  - Public `chapter_idx` and `reader: ReaderState` fields for direct access;
    save `chapter_idx` + `reader.anchor_byte` to persist the reading position

## 2026-08-01 (Fix ESP startup crash and ChannelIFace lifetime)

- `examples/ereader_ui.rs`: moved `TimerGroup` + `SoftwareInterruptControl` init and `esp_rtos::start()` to immediately after `esp_hal::init()`, before the first `EmbassyTimer::after()` call — the embassy time driver must be running before any timer await or the device panics with `time_driver.as_mut() failed: NoneError`
- `src/hardware.rs`: fixed `ChannelIFace` lifetime/type parameters — esp-hal 1.1 defines it as `ChannelIFace<'a, S: TimerSpeed>`, so `EspHardware` now bounds `C: ChannelIFace<'d, LowSpeed>` with `LowSpeed` imported from `esp_hal::ledc`

## 2026-08-01 (HardwareAccess trait abstraction)

- `src/hardware.rs` (new): hardware abstraction layer with:
  - `FontSize`, `BacklightLevel`, `Orientation` enums (feature-agnostic, shared by both platforms)
  - `Orientation` carries coordinate-mapping methods (`phys_to_logical`, `logical_to_phys`, `logical_size`) and `PANEL_W`/`PANEL_H` constants
  - `HardwareAccess` trait with getters and setters for all four values (font size, backlight, orientation, current time)
  - `SimHardware` (feature=simulator): in-memory state, `current_time_secs()` via `SystemTime::now()`
  - `EspHardware<'d, C: ChannelIFace>` (feature=esp): owns LEDC channel + RTC; `set_backlight_level` drives LEDC duty cycle, `current_time_secs` reads RTC; initial backlight applied on construction
- `src/lib.rs`: added `pub mod hardware;`
- `examples/ereader_ui.rs`:
  - Removed inline `EspOrientation` enum; replaced throughout with `ereader::hardware::Orientation`
  - Replaced `cur_font_idx`, `cur_bl_idx`, `orientation` variables with a single `hw: EspHardware<_>` on the ESP path and `hw: SimHardware` on the simulator path
  - Event loop now routes font/backlight/orientation toggles through `hw.set_*(...)` and calls `save_settings` from the `hw.*_level().to_index()` accessors
  - Simulator `sync_time` button calls `hw.current_time_secs()` instead of directly querying `SystemTime`
  - Removed now-unused `SCREEN_W`/`SCREEN_H` constants

## 2026-07-31 (ereader_ui NTP time sync on device)

- `examples/ereader_ui.rs`: ESP main function converted to async and wired up WiFi + NTP time sync
  - `#[main]` / `fn main() -> !` changed to `#[esp_rtos::main]` / `async fn main(spawner: Spawner) -> !`
  - `esp_alloc::heap_allocator!(size: 72 * 1024)` added alongside the PSRAM allocator — WiFi stack requires SRAM heap
  - `Rtc::new(peripherals.LPWR)` added; RTC uptime used as embassy-net random seed and to store the synced time
  - `TimerGroup` + `SoftwareInterruptControl` initialised; `esp_rtos::start()` called before WiFi init
  - WiFi station config (`WIFI_SSID`/`WIFI_PASS` from build-time env vars, fallback to placeholder) passed to `esp_radio::wifi::new()`
  - Embassy-net stack created; `net_task` and `wifi_connection` tasks spawned via `spawner`
  - At startup: blocks on `stack.wait_config_up().await`, then calls `query_ntp(stack).await` — on success sets the RTC, updates the `"time"` label and triggers a redraw
  - Pressing the Sync Time button re-runs the NTP query and updates the label without reinitializing WiFi
  - `delay.delay_millis(50)` in the main loop replaced with `EmbassyTimer::after(Duration::from_millis(50)).await`; display power-on delays likewise converted to embassy timers
  - `format_time_utc` de-gated from `simulator`-only so it's available on both targets
  - `Cargo.toml`: `static-cell` corrected to `static_cell` (crate name on crates.io uses underscore)

## 2026-07-31 (ereader_ui orientation on device)

- `examples/ereader_ui.rs`: orientation toggle now rotates the display on the ESP device
  - Added `EspOrientation` enum (`Port`, `Land`, `RPort`, `RLand`) with helpers:
    - `logical_to_phys(lx, ly)` — pixel mapping for `draw_iter` (replaces hardcoded portrait-only formula)
    - `phys_to_logical(tx, ty)` — touch coordinate inverse; replaces hardcoded `lx = 539-ty, ly = tx`
    - `logical_size()` — returns (w, h) for the logical screen; portrait = 540×960, landscape = 960×540
  - `Rgb565ToGray4` gains an `orientation` field; `draw_iter` and `OriginDimensions::size()` are now orientation-aware
  - On orientation command: `bridge.orientation` updated, `scene.bounds` resized, `mark_layout_dirty()` triggers full redraw
  - Orientation persisted to NVS key 12; loaded at startup and applied before the first render
  - Touch coordinate calculation simplified to a single `orientation.phys_to_logical(tx, ty)` call
  - `needs_full_refresh` check uses `orientation.logical_size()` instead of the fixed `SCREEN_W`/`SCREEN_H` constants
  - Dialog height is now dynamic: `layout_dialog` passes unconstrained space (4000px) to `layout_vbox`, measures children's bottom edge, then centers the dialog at the correct height — fixes Close button falling out of dialog at Large font size

## 2026-07-31 (ereader_ui persistent settings)

- `examples/ereader_ui.rs`: font size and backlight level now persist across reboots via NVS flash (ESP only)
  - Copied `FlashAdapter`, `block_on` no-std executor, and sequential_storage pattern from `ereader_full`
  - `load_settings()` / `save_settings()` use keys 10 (font) and 11 (backlight) to avoid collisions with `ereader_full`'s 0–4 key range; defaults are Medium (1) and High (2)
  - Settings are loaded at startup and applied to `theme.font`/`theme.bold_font` and `bl_ch.set_duty()` before the first render
  - `save_settings()` is called immediately after each font size or backlight change
  - Font and backlight commands now track a numeric index (`cur_font_idx`, `cur_bl_idx`) so both the theme and flash store use the same value

## 2026-07-31 (ereader_ui backlight toggle)

- `examples/ereader_ui.rs`: Backlight toggle group now controls the actual display backlight on the ESP device
  - LEDC initialised on GPIO11 (Channel0, Timer0, 8-bit duty, 1 kHz) — same pin and config as `ereader_full`
  - Starts at 100% (High) matching the toggle group default selection
  - Off → 0%, Low → 25%, High → 100%; change takes effect immediately via `bl_ch.set_duty()` with no scene redraw needed
  - Added `esp_hal::ledc`, `esp_hal::gpio::DriveMode`, and `esp_hal::time::Rate` imports (ESP only)

## 2026-07-31 (ereader_ui font size on device)

- `examples/ereader_ui.rs`: font size toggle now works on the ESP device
  - `theme` made `mut` in ESP `main`; `click_at` return value is now inspected in the touch handler
  - When `target == "font_size"`, `theme.font`/`theme.bold_font` are updated and `scene.mark_layout_dirty()` triggers a full-screen refresh (which automatically uses the ghost-clear path)
  - Font mapping matches the simulator: Small → `FONT_6X10`, Medium → `FONT_9X15`/`FONT_9X15_BOLD`, Large → `FONT_10X20`

## 2026-07-31 (ereader_ui dialog hide fix + logging)

- `examples/ereader_ui.rs`: dialog now disappears correctly when closed on the e-paper device
  - Root cause: e-paper cannot drive dark pixels (dialog border/text) back to white in a single `BlackOnWhite` flush; requires a ghost-clear pass first
  - Full-screen dirty redraws (dialog show/hide, orientation change, font change) now do `fill(0x0F)` + `flush(WhiteOnBlack)` before drawing content, then `flush(BlackOnWhite)` — matching the `ereader_full` page-turn pattern
  - Partial dirty redraws continue to use a single `BlackOnWhite` flush (no visible flash)
  - ESP logger switched from `init_logger_from_env()` to `init_logger(LevelFilter::Info)` so `info!()` output is always visible on the USB serial console (previously silently filtered when `ESP_LOG` env var was not set at compile time)
  - `draw_dialog`: fill now uses `Rgb565::WHITE` explicitly (was `e.theme.bg`) with a comment confirming it runs before children draw

## 2026-07-31 (ereader_ui sync time)

- `examples/ereader_ui.rs`: Sync Time button in settings dialog now sets the time label to the real system clock
  - `localtime: u64` initialised to 0 before the main loop; time label starts as `--:-- --`
  - Pressing Sync Time calls `SystemTime::now()`, stores unix timestamp in `localtime`, updates `view.title` on the `"time"` label, and calls `mark_layout_dirty` to trigger a redraw
  - `format_time_utc(unix_secs)` converts a unix timestamp to a 12-hour `H:MM AM/PM` string (UTC)
  - Refactored click dispatch from `if let Some((_, Action::Command(cmd)))` to `if let Some((target, action))` so both `Action::Command` (toggle groups) and `Action::Generic` (buttons) can be handled in the same block

## 2026-07-31 (ereader_ui font size selector)

- `examples/ereader_ui.rs`: font size toggle in settings dialog now changes the content font
  - Added `FONT_6X10` (Small) and `FONT_10X20` (Large) imports alongside existing `FONT_9X15`/`FONT_9X15_BOLD`
  - `theme` made mutable in simulator `main`; after a `font_size` click the matching `theme.font`/`theme.bold_font` are swapped and `mark_layout_dirty` triggers a full redraw
  - Small → `FONT_6X10`, Medium → `FONT_9X15` + `FONT_9X15_BOLD`, Large → `FONT_10X20`
  - `draw_content` already derives `char_w`/`char_h` from `e.theme.font` so word-wrap and line spacing adapt automatically

## 2026-07-31 (ereader_ui orientation resize)

- `examples/ereader_ui.rs`: selecting an orientation in the Settings dialog now resizes the simulator window
  - `make_scene` takes explicit `w: i32, h: i32` params instead of using `SCREEN_W`/`SCREEN_H` constants; both ESP and simulator pass the constants; simulator passes updated dims on resize
  - Simulator `main` collects events into a `Vec` before iterating (required so `window` can be reassigned inside the same loop body)
  - On orientation click: reads the label string from `Action::Command(cmd)`; `"Land"/"R.Land"` → 960×540, anything else → 540×960; if dims changed, updates `scene.bounds`, calls `scene.mark_layout_dirty()`, recreates `SimulatorDisplay` and `Window`
  - `layout_dialog`: replaced hardcoded `SCREEN_W`/`SCREEN_H` with `pass.space.w`/`pass.space.h` so the dialog centers correctly after a resize
  - `content` view: added `layout: Some(layout_std_panel)` so it resizes to fill the flex space properly on any screen size; removed stale manual `bounds`
  - Added `Action` and `layout_std_panel` to imports

## 2026-07-30 (ereader_ui settings dialog expanded)

- `examples/ereader_ui.rs`: added three items to the settings dialog
  - Orientation toggle group: `[Port | Land | R.Port | R.Land]` (Portrait selected by default)
  - "Sync Time" button
  - Battery status label: "Battery: 85%  (Charging)" (fake data)
  - `DIALOG_H` increased 240 → 340 px to accommodate 10 stacked items

## 2026-07-30 (ereader_ui settings dialog)

- `examples/ereader_ui.rs`: pressing Settings now shows a centered modal dialog
  - Dialog is added as the last child of the root vbox so it renders on top of all other views; starts hidden (`visible: false`)
  - `draw_dialog`: fills white background, double-stroke border for visual weight
  - `layout_dialog`: centers the dialog on screen (60 × 340 px, 420 × 240), then delegates child layout to `layout_vbox`
  - Dialog contains: "Settings" title label, "Font Size" label + `[Small|Medium|Large]` toggle group (Medium default), "Backlight" label + `[Off|Low|High]` toggle group (High default), Close button
  - `handle_click` callback: shows dialog on Settings tap, hides it on Close tap; wired into the simulator's `handlers` vec (`Vec<Callback>`)
  - `#[macro_use]` added to the ESP `extern crate alloc` so `vec![]` works in shared `make_scene` code on the no_std target
  - Removed spurious `use fontdue::layout::HorizontalAlign` import

## 2026-07-30 (ereader_ui logging)

- `Cargo.toml`: added `env_logger = "0.11"` as optional dep; enabled via `simulator` feature alongside `dep:log`
- `examples/ereader_ui.rs`: wired up logging for both targets
  - Simulator: `env_logger` initialised with default level `info` (overridable via `RUST_LOG`); run with `RUST_LOG=info cargo sim --example ereader_ui`
  - ESP: `esp-println` with `log-04` feature was already present; `init_logger_from_env()` already called in ESP `main`; output goes to the USB serial console; log level set at compile time via `ESP_LOG`
  - `make_scene()` calls `scene.dump()` + `log::info!` (uses `log` crate directly so it compiles on both std and no_std targets)

## 2026-07-30 (ereader_ui layout)

- `examples/ereader_ui.rs`: replaced three-button placeholder with full e-reader chrome layout
  - **Top bar** (gray, `layout_hbox`): Settings button, time label "10:42 AM", battery label "85%", book title "Sherlock Holmes"
  - **Content area** (white, fills remaining height): word-wrapped book text from *The Adventures of Sherlock Holmes* — `next_line()` helper splits on word boundaries without heap allocation
  - **Bottom bar** (gray, `layout_hbox`): chapter label and page label; horizontal rule drawn at top edge
  - Top/bottom bars draw gray fill + border line via custom draw functions; content fills its entire bounds in white
  - All data is fake/static; no ESP-path changes

## 2026-07-30 (ereader_ui)

- `Cargo.toml`: added `iris-ui = "0.1.0"` as optional dep; added `"dep:iris-ui"` to both `esp` and `simulator` features
- `examples/ereader_ui.rs`: new example using iris-ui, runs on both ESP and simulator
  - 540×960 portrait orientation matching `ereader_full`
  - `make_scene()` builds a full-screen panel (`layout_vbox`) with three buttons: Previous, Next, Settings
  - Simulator: `SimulatorDisplay<Rgb565>` at 540×960 + SDL2 `Window`; mouse click dispatched via `click_at`
  - ESP: `Rgb565ToGray4<'a>` bridge wraps the Gray4 e-paper display; converts Rgb565 luminance to 4-bit gray and rotates coordinates 90° CCW (portrait → physical landscape); flushes only when `dirty_rect` is non-empty
- `README.md`: added `ereader_ui` to example table; added combined simulator prerequisites section; added `ereader_ui` simulator section

## 2026-07-30

- `Cargo.toml`: added `simulator` feature; added `embedded-graphics-simulator = "0.8"` optional dep
- `examples/ereader_full.rs`: refactored to run on both ESP and the SDL2 desktop simulator depending on enabled features
  - Added `EreaderDisplay` trait abstracting `put_pixel`/`fill_display`/`clear_display`/`flush_display` over both backends
  - Made `RotatedDisplay` generic over any `DrawTarget<Color = Gray4> + OriginDimensions` (was concrete `Display<'hw>`)
  - Made `draw_content`, `render_page`, `update_header_only`, `update_footer_only`, `restore_after_dropdown`, `fast_scroll` generic over `EreaderDisplay`
  - Moved `time`/`soc`/`charging` out of `render_page` into call sites; added `esp_display_status` helper for ESP
  - Defined `DrawMode` locally (shared); ESP impl maps to `ereader::driver::display::DrawMode`
  - Gated ESP-specific code (RTC, LEDC, touch, flash, deep sleep, battery) under `#[cfg(feature = "esp")]`
  - Added `SimDisplay` struct (wraps `SimulatorDisplay<Gray4>` + SDL2 `Window`) with `EreaderDisplay` impl
  - Added simulator `main`: portrait default, keyboard navigation (Left/Right/Space/Backspace), ~30 fps event loop
  - Run simulator with: `cargo sim --example ereader_full`
- `.cargo/config.toml`: added `sim` alias (`run --no-default-features --features simulator --target aarch64-apple-darwin`)

## 2026-07-28 17:31

- `fonts/NoticiaText-Regular.ttf`: replaced incomplete subset (missing D, E, J, K, M, U, W, X, Z) with full font downloaded from Google Fonts (113 KB); fixes missing uppercase glyphs in rendered text
- `examples/ereader_full.rs`: unified portrait and landscape font sizes — portrait now uses the same px values as landscape (was smaller before)
- `src/layout.rs`: replaced `vec![]` macro with explicit `Vec::new()` + `push()` to compile under `no_std` without importing the alloc macro

## 2026-07-28 (epub_test local run)

- Added `esp` feature (default-on) to `Cargo.toml`; all ESP-specific deps are now optional and gated by it
- `src/lib.rs`: `#![no_std]` and `extern crate alloc` now conditioned on `esp` feature
- `src/{epub,layout,reader}.rs`: dual `alloc`/`std` imports so library compiles in both std and no_std contexts
- `src/lib.rs`: `pub mod driver` gated behind `#[cfg(feature = "esp")]`
- `build.rs`: ESP linker args (`-Tlinkall.x`, `--error-handling-script`) gated behind `esp` feature
- `.cargo/config.toml`: `rustflags = -nostartfiles` moved to `[target.xtensa-esp32s3-none-elf]`; removed global `build-std`; added `esp-build`/`esp-run` aliases for device builds
- `examples/epub_test.rs`: split into `run_tests()` helper + two conditional entry points; runs locally with `cargo run --example epub_test --no-default-features --target aarch64-apple-darwin`

## 2026-07-28

- Switched ereader book from Moby Dick to The Adventures of Sherlock Holmes (Project Gutenberg #1661, downloaded as `examples/sherlock_holmes.epub`)
- Changed font from Georgia to Noticia Text (`src/font.rs` now loads `NoticiaText-Regular.ttf`)
