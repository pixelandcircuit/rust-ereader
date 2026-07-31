# Changes

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
