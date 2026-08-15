# Architecture

This document describes how the `ereader` codebase is organized and how data flows from a
book file on disk to pixels on the e-paper panel (or the desktop simulator window).

The project targets the LilyGO T5S3 Pro board: an ESP32-S3 driving a 960x540 ED047TC1
e-paper panel over an 8-bit I8080 parallel bus, with a GT911 capacitive touch controller,
an SD card, and a PCA9555 I/O expander + TPS65185 PMIC for panel power. It also builds as a
desktop simulator for fast iteration without hardware.

## Dual build targets: `esp` vs `simulator`

There is no runtime hardware-abstraction trait object; the split is done with Cargo
features and `#[cfg(...)]` gates, resolved at compile time:

- **`esp`** (default): `#![no_std]`, Xtensa target (`xtensa-esp32s3-none-elf`), built on
  `esp-hal`/`esp-alloc`/`embassy` and flashed with `espflash`.
- **`simulator`**: plain `std`, host target, renders into an SDL2-backed
  `embedded_graphics_simulator` window (`cargo sim` alias in `.cargo/config.toml`).

There is a single example/composition-root binary, `examples/ereader_ui.rs`, compiled twice
— once per feature set — with two separate `main` entry points (`fn main()` for the
simulator, `#[embassy_executor::main] async fn main` for ESP). Two abstractions keep the
app logic shared between targets:

- **`HardwareAccess` trait** (`src/hardware.rs:183`) — implemented by `SimHardware` (std,
  in-memory/`std::fs`-backed) and `EspHardware` (no_std, real peripherals/NVS flash). All
  app logic is written against `&dyn HardwareAccess` so it doesn't care which target it's
  running on.
- **`Orientation` enum** (`src/hardware.rs:90`) — the single source of truth for
  portrait/landscape coordinate remapping, shared by touch input and display drawing on
  both targets.

On ESP, an additional `Rgb565ToGray4` bridge (`examples/ereader_ui.rs:974`) adapts the
UI framework's RGB565 draw calls into the panel's native 4-bit grayscale framebuffer,
performing the portrait pixel remap. On the simulator, the same UI draws directly into an
RGB565 SDL window — no grayscale conversion needed.

The UI itself is built with **`iris-ui`**, a small retained-mode GUI framework (Scene/View
tree over `embedded-graphics`) that lives in a local sibling crate (`../rust-embedded-gui`)
— not egui, not Slint. `src/h_spacer.rs` and `src/truncating_label.rs` are `iris-ui` view
helpers used by the app.

## Layers

### 1. Hardware / driver layer

Everything here is either cross-platform (`src/hardware.rs`) or ESP-only
(`src/driver/*`, gated behind the `esp` feature in `src/lib.rs`).

- **`src/hardware.rs`** — the `HardwareAccess` trait plus its two implementations.
  - `SimHardware` — no-op/in-memory persistence for desktop runs.
  - `EspHardware` — owns the LEDC backlight channel, RTC, buttons, battery reading; reads
    the SD card over SPI2 via `embedded-sdmmc` (SPI2 is shared with the LoRa radio's CS
    line, which must be deasserted first, plus a manual 74-clock SD power-up preamble —
    see the note on SPI2 sharing below).
  - Settings/bookmarks persist to NVS flash via `sequential-storage` over a hand-rolled
    `FlashStorage` adapter, driven synchronously with a no-op-waker `block_on`.
  - Font size, backlight, orientation, timezone, and reading position also mirror into
    RTC fast-memory registers so they survive deep sleep without wearing out flash.
- **`src/driver/mod.rs`** — driver module root; re-exports `Display`, `DrawMode`,
  `PinConfig`, `Gt911`; defines the board's pin bindings.
- **`src/driver/ed047tc1.rs`** — low-level ED047TC1 panel protocol: I8080 parallel bus +
  RMT-driven row clock + PCA9555 I/O expander + TPS65185 power sequencing
  (`power_on`/`power_off` with polled PWRGOOD), and the per-row DMA drive
  (`frame_start`/`output_row`/`frame_end`).
- **`src/driver/display.rs`** — the waveform/LUT engine and 4bpp framebuffer
  (`Display<'a, I>`). Builds a 64 KB drive-code LUT per refresh (`update_lut`) for one of
  five `DrawMode`s (full-quality 15-frame `BlackOnWhite`/`WhiteOnWhite`/`WhiteOnBlack`
  waveforms, or fast 4-frame `Fast`/`FastClear` page-turn waveforms). Tracks dirty rows for
  partial refresh and masks out-of-rect columns during region flushes so partial updates
  don't disturb surrounding pixels.
- **`src/driver/graphics.rs`** — `embedded-graphics::DrawTarget` impl for `Display` (raw
  Gray4 pixel writes), plus a separate low-level rotation helper distinct from the app's
  `Orientation` enum.
- **`src/driver/gt911.rs`** — polling driver for the GT911 touch controller: writes the
  184-byte config block to bring up an unprogrammed chip, then reads touch point + face
  button state.
- **`src/driver/rmt.rs`** — thin wrapper around the ESP32-S3 RMT peripheral, used purely to
  generate the precisely-timed row-clock pulse train for the e-paper panel. (Not related to
  LoRa, despite sharing SPI2 with the LoRa radio elsewhere on the board.)

### 2. Data / model layer

- **`src/book.rs`** — `Book` trait (`spine()`, `chapter_text(id)`), implemented by
  `TxtBook` and `HtmlBook`. Both decode text losslessly (UTF-8 with Latin-1 fallback) and
  encode heading/bold/italic runs as sentinel bytes directly in the returned text, which
  the layout/render stages interpret downstream.
- **`src/epub.rs`** — a hand-rolled ZIP/EPUB parser (no external zip crate): scans for the
  End-Of-Central-Directory record, walks the central directory, decompresses entries with
  `miniz_oxide`. Parses `container.xml` and the OPF manifest/spine with `xmlparser`, and
  implements `Book` by decompressing one XHTML chapter at a time on demand (not the whole
  archive up front — this is what keeps memory bounded for large EPUBs).
- **`src/reader.rs`** — `ReaderState` (one chapter's text + its paginated `Layout` +
  current page + a byte-offset anchor used to re-find position after a font-size-driven
  relayout) and `BookSession` (adds spine navigation: next/prev/goto chapter, restore from
  a saved position without redundant layout work).

### 3. Layout / rendering pipeline

Rendering a page flows through these stages:

```
book/epub (sentinel-tagged text)
  -> reader::BookSession / ReaderState   (pagination via layout::layout_chapter)
  -> bookview::update_content            (pushes page text + font size into BookState)
  -> iris-ui scene draw pass
  -> bookview::draw_book_content -> render_ttf_text   (word-wrap + fontdue rasterization)
  -> iris-ui DrawingContext
  -> [ESP only] Rgb565ToGray4 bridge -> Display framebuffer -> ed047tc1/rmt -> panel
  -> [simulator] SimulatorDisplay<Rgb565> window directly
```

- **`src/layout.rs`** — `layout_chapter()`, the pure pagination engine: a byte-level scan
  over the UTF-8 text that tracks heading/bold/italic state via the sentinel bytes, measures
  glyph widths (with a small per-style ASCII cache), breaks lines on `\n`, adds extra
  paragraph spacing on `\n\n`, and emits page byte-ranges.
- **`src/bookview.rs`** — the rendering half, kept pixel-exact with `layout.rs`.
  `layout_cfg()` builds a `LayoutConfig` from real font metrics and the actual content-view
  pixel bounds (must match layout's assumptions exactly, or text clips at page edges).
  `render_ttf_text()` draws glyph runs, switching fonts at sentinel boundaries.
- **`src/font.rs`** — a thin fontdue wrapper. `AppFonts` bundles `&'static fontdue::Font`
  references (not owned `Font`s) so the font set threads cheaply through call stacks —
  deliberately structured this way after an earlier out-of-memory issue caused by parsing
  multiple font instances.
- **`src/h_spacer.rs`, `src/truncating_label.rs`** — `iris-ui` view helpers: a flexible
  spacer and a width-aware ellipsis-truncating label (used for book titles in the UI).

### 4. App state / composition root

- **`src/appstate.rs`** — `AppState`, the single top-level app struct: the current `Book`,
  its `BookSession`, the derived `LayoutConfig`, the `iris-ui` `Scene`, `AppFonts`, theme,
  and refresh-quality counters (used to decide when to force a full-quality refresh after a
  run of fast partial ones). `book_from_data()` dispatches on file extension to build the
  right `Book` impl, falling back to an error-message `TxtBook` if EPUB parsing fails.
- **`examples/ereader_ui.rs`** — the composition root for both targets. There's no separate
  reading/settings/sleep state enum; screen modes are `iris-ui` dialog views toggled
  visible/hidden (settings, library, battery, error, loading, fast-scroll). On ESP, `main`
  sets up allocators, the shared I2C bus, `Display`/`Gt911`, backlight, restores settings
  from RTC memory (deep-sleep wake) or NVS (cold boot), then runs an Embassy task set:
  battery polling, time sync, and async book loading (loads the new book and frees the old
  one before layout, which is what avoids the double-allocation OOM on large EPUBs), plus a
  one-shot WiFi/NTP sync. Idle timers drive two tiers of sleep (light sleep, then deep
  sleep) via `HardwareAccess`.

## Build / tooling

- **`Cargo.toml`** — feature `esp` (default) pulls in `esp-hal`, `embassy`, flash/NVS,
  WiFi, and SD card deps; feature `simulator` pulls in `embedded-graphics-simulator`
  (SDL2). `iris-ui` is a local path dependency (`../rust-embedded-gui`) used by both.
  Always-on deps: `fontdue` (TTF rasterization), `miniz_oxide` (DEFLATE for EPUB),
  `xmlparser` (no_std XML for OPF/container.xml parsing). Release profile is tuned for
  constrained flash/PSRAM (`opt-level = "s"`, `lto = "fat"`, `codegen-units = 1`).
- **`build.rs`** — emits ESP linker arguments and generates test grayscale image data.
- **`partitions.csv`** — ESP32-S3 partition table: NVS (settings/bookmarks), factory app
  (4 MB), and a currently-unused SPIFFS storage partition reserved for future assets.
- **`rust-toolchain.toml`** / **`.cargo/config.toml`** — pin the espup-managed Xtensa
  toolchain; define the `esp-build`/`esp-run` (device) and `sim`/`test-sim` (host) cargo
  aliases.
- **`changes.md`** — chronological development log; the best source for the *why* behind
  non-obvious decisions referenced above (waveform/LUT tuning, OOM fixes, font memory
  crises, partial-refresh dirty-rect logic).

## Key file:line reference index

| Concept | Location |
|---|---|
| `HardwareAccess` trait | `src/hardware.rs:183` |
| `Orientation` (portrait remap) | `src/hardware.rs:90` |
| `EspHardware` / `SimHardware` | `src/hardware.rs:643` / `src/hardware.rs:234` |
| `AppState` | `src/appstate.rs:21` |
| `Display<'a, I>` (framebuffer + LUT) | `src/driver/display.rs:78` |
| `DrawMode` (waveforms) | `src/driver/display.rs:31` |
| `update_lut` | `src/driver/display.rs:550` |
| `ED047TC1<'a, I>` | `src/driver/ed047tc1.rs:88` |
| `Rmt` (RMT/row clock) | `src/driver/rmt.rs:9` |
| `Gt911` | `src/driver/gt911.rs:23` |
| `Book` trait | `src/book.rs:16` |
| `EpubArchive` | `src/epub.rs:147` |
| `ReaderState` / `BookSession` | `src/reader.rs:22` / `src/reader.rs:100` |
| `layout_chapter` | `src/layout.rs:66` |
| `layout_cfg` / `render_ttf_text` | `src/bookview.rs:82` / `src/bookview.rs:232` |
| `AppFonts` | `src/font.rs:7` |
| `Rgb565ToGray4` bridge | `examples/ereader_ui.rs:974` |
| Simulator `main()` | `examples/ereader_ui.rs:638` |
| ESP `main()` | `examples/ereader_ui.rs:1300` |

## Known documentation gaps

`README.md` still references `examples/ereader_full.rs` and `examples/epub_test.rs`, and a
`library/` directory nested under `examples/`. Neither exists anymore — the current tree
has a single `examples/ereader_ui.rs`, and `library/` lives at the repo root. Worth fixing
in the README separately.
