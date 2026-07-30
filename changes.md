# Changes

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
