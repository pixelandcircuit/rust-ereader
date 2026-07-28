# Changes

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
