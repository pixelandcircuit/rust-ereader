# Changes

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
