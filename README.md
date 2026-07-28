# ereader

An e-reader application for the [Lilygo T5 E-Paper S3 Pro](https://www.lilygo.cc/products/t5-e-paper-s3-pro) (ESP32-S3, 9.7" ED047TC1 e-paper display). Reads EPUB files embedded directly in firmware at compile time.

## Hardware

| Component | Detail |
|-----------|--------|
| MCU | ESP32-S3 |
| Display | 9.7" ED047TC1 (960×540, 4-bit grayscale) |
| Touch | GT911 capacitive touchscreen |
| Battery | BQ27220 fuel gauge, BQ25896 charger |

## Prerequisites

Install the ESP Rust toolchain via [espup](https://github.com/esp-rs/espup):

```sh
cargo install espup
espup install
```

Install the flash/monitor tool:

```sh
cargo install espflash
```

Activate the ESP environment (run once per shell session, or add to your shell profile):

```sh
. ~/export-esp.sh
```

## Building and flashing

The default Cargo target is `xtensa-esp32s3-none-elf`. Use the `esp-build` / `esp-run` aliases which include the required `-Z build-std=alloc,core` flag:

```sh
# Build only
cargo esp-build --example ereader_full

# Build and flash to device (opens serial monitor after flashing)
cargo esp-run --example ereader_full
```

## Changing the book

The active book is embedded at compile time in `examples/ereader_full.rs`:

```rust
const EPUB_DATA: &[u8] = include_bytes!("sherlock_holmes.epub");
```

Replace the filename with any EPUB placed in the `examples/` directory, then rebuild and reflash.

## Changing the font

The font is set in `src/font.rs`:

```rust
static FONT_DATA: &[u8] = include_bytes!("../fonts/NoticiaText-Regular.ttf");
```

Available fonts in `fonts/`:

- Alegreya (Regular / Italic / Bold)
- AtkinsonHyperlegible (Regular / Italic / Bold)
- CrimsonPro (Regular / Italic / Bold)
- Georgia
- Literata (Regular / Italic / Bold)
- NoticiaText (Regular / Italic / Bold) ← current
- Vollkorn (Regular / Italic / Bold)

## Examples

| Example | Description |
|---------|-------------|
| `ereader_full` | Full application: EPUB reader with touch, backlight, battery display, flash persistence |
| `ebook` | Minimal EPUB reader skeleton |
| `font_compare` | Side-by-side font comparison across six typefaces |
| `epub_test` | EPUB library smoke test — runs on device **or** locally (see below) |

## Running epub_test locally (no device needed)

`epub_test` exercises the EPUB, layout, and reader modules without any display hardware. It can run on your development machine:

```sh
cargo run --example epub_test --no-default-features --target aarch64-apple-darwin
```

Adjust the target triple for your machine:

| Machine | Target triple |
|---------|---------------|
| Apple Silicon Mac | `aarch64-apple-darwin` |
| Intel Mac | `x86_64-apple-darwin` |
| Linux x86-64 | `x86_64-unknown-linux-gnu` |

## Project structure

```
src/
  epub.rs       — ZIP/EPUB parser, spine extraction, HTML-to-plaintext stripping
  layout.rs     — Word-wrap and pagination engine
  reader.rs     — Stateful reader (current page, relayout on font-size change)
  font.rs       — TTF rasterizer (fontdue) with Gray4 blending
  driver/       — ESP32-S3 display (ED047TC1), touch (GT911), RMT/DMA drivers
examples/
  ereader_full.rs  — Main application binary
  epub_test.rs     — Cross-platform library smoke test
fonts/           — Embedded TTF/OTF font files
```

## Cargo features

| Feature | Default | Description |
|---------|---------|-------------|
| `esp` | yes | Enables all ESP32-S3 hardware dependencies. Disable with `--no-default-features` for local/native builds. |

## Flash partition layout

Defined in `partitions.csv`. Sequential-storage is used for persisting reading position, font size, orientation, and backlight level across deep-sleep cycles.
