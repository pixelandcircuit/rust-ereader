# Changes

## 2026-08-14

Add Embassy background tasks for battery polling and WiFi/NTP sync.

- `src/driver/ed047tc1.rs`, `display.rs`, `mod.rs`: Refactored I2C bus out of the display driver. `ED047TC1<'a, I>` and `Display<'a, I>` are now generic over `I: embedded_hal::i2c::I2c`. GPIO39/40 removed from `PinConfig` and `pin_config!` macro; the shared bus is constructed in `main` instead. `DISPLAY_WIDTH`/`DISPLAY_HEIGHT` promoted to module-level constants in `display.rs` to avoid generic-dependent const expressions.
- `src/driver/graphics.rs`: Updated `DrawTarget` and `OriginDimensions` impls to `Display<'a, I>`.
- `Cargo.toml`: Added `embassy-sync = "0.8"` and `critical-section = "1"` as explicit optional deps (pinned to match `esp-radio`'s transitive version).
- `examples/ereader_ui.rs`:
  - Builds a `static I2C_BUS: StaticCell<critical_section::Mutex<RefCell<I2c<'static, Blocking>>>>` in `main` and hands one `CriticalSectionDevice` handle to the display driver and another to the new `battery_task`.
  - `battery_task`: `#[embassy_executor::task]` — reads BQ27220 every 10 s, signals `BATTERY_RESULT`.
  - `wifi_task` + `do_ntp_sync`: `#[embassy_executor::task]` — connects to AP, syncs NTP, disconnects; waits on `WIFI_SYNC_REQUEST` for on-demand re-syncs. Replaces the blocking WiFi/NTP code that previously ran synchronously on boot.
  - Main loop now `try_take()`s `BATTERY_RESULT` and `WIFI_SYNC_RESULT` each iteration.
  - `sync_time` button now signals `WIFI_SYNC_REQUEST` (was a commented-out stub).
  - `Rgb565ToGray4<'a, I>` made generic to propagate the I2C type parameter.

## 2026-08-13 (2)

Fix: device forgot which book and position it was at after a hard reset.

Three bugs, all fixed:
- `examples/ereader_ui.rs`: `save_last_filename()` was only called before deep sleep; now also called when a book is opened so the filename survives a hard reset.
- `examples/ereader_ui.rs`: `load_last_filename()` was gated on `is_sleep_wakeup`; now called on every boot (cold reset and sleep wakeup alike).
- `examples/ereader_ui.rs`: on cold boot, `saved_chapter`/`saved_anchor` came from `load_cold_boot_position()` (the embedded book's bookmark), not the SD card book's bookmark. Now after the SD card book is loaded its own bookmark is looked up via `hw.load_bookmark()` and overwrites the position.

## 2026-08-13

Face button (circle below screen) wired up via GT911 key area; press/release logging for all three buttons.

- `src/driver/gt911.rs`: added `read_input()` returning `(Option<(u16,u16)>, bool)` — reads touch point and `have_key` (bit 4 of status) in a single I2C transaction. Added `key_pressed()` for polling the key state inside a hold loop. `read_touch()` now delegates to `read_input()`.
- `src/driver/display.rs`: added `read_touch_and_key()` and `gt911_key_pressed()` delegating to the GT911 driver.
- `examples/ereader_ui.rs`: replaced `Option<bool>` button state with a `Btn` enum (`Prev`/`Next`/`Face`). Each loop iteration calls `read_touch_and_key()` once to get both touch and face-button state without a redundant status read. Logs `"BOOT/SIDE/FACE button pressed[wake]"` and `"... released"`. Face button hold loop polls `gt911_key_pressed()` every 10 ms.

## 2026-08-12 19:20

Fix battery dialog showing hardcoded 85% / "Not charging".

`EspHardware::battery_info()` was returning hardcoded values. The board has a
BQ27220 fuel gauge at I2C 0x55 (on the same GPIO39/40 bus as the GT911 touch
controller). Correct registers: voltage at 0x08 (mV, 16-bit LE), average
current at 0x14 (mA, signed 16-bit LE), state-of-charge at 0x1E (%, 16-bit).
`is_charging` is derived from current_ma > 0. The dialog now reads live values
from the fuel gauge immediately before opening.

## 2026-08-12 13:30

Skip full-screen clear on fast-paging panel first show.

When the fast-scroll panel first appears, `mark_layout_dirty()` was setting
`dirty_rect` to the full scene bounds, triggering a full-screen FastClear + Fast
waveform pass before the tiny dialog was drawn.  Fixed by running `layout_scene`
first (so the panel has its computed centred position), then resetting `dirty_rect`
and calling `mark_dirty_view(&FAST_SCROLL_PANEL_ID)` to rederive bounds from the
panel alone.  The display is now only refreshed over the panel region on every
update — no full-screen clear at all during fast paging.

## 2026-08-12 13:15

Use 4-frame waveforms for fast-paging counter updates.

While the user holds a button the fast-paging loop now always uses
`DrawMode::FastClear` + `DrawMode::Fast` (4+4 frames) instead of the full
15+15 frame waveform.  For the small counter dialog (~100 active rows) this
cuts per-update display time from ~158 ms to ~90 ms.  Quality doesn't matter
during fast paging because the full screen gets a proper 15-frame refresh the
moment the button is released.  Removed the `partial_refresh_count` and
`full_quality_count` bookkeeping from the hold loop — those counters are only
relevant to the main render path.

## 2026-08-12 12:45

Fix dark screen on boot caused by incorrect Fast waveform LUT reversal.

`update_lut` was computing `k_eff = frame_count - k` (4 - k for Fast). With only 4
frames this produced k_eff values 4→1, so white background pixels (value 15) were never
VCOM'd and kept the default `0x55` (drive-dark) code throughout all Fast frames —
driving the entire screen dark. Fixed by introducing `FULL_FRAME_COUNT = 15` as the
fixed reversal offset for all reversed modes (`BlackOnWhite`, `WhiteOnWhite`, `Fast`).
Fast now produces k_eff 15→12, correctly VCOM-ing white pixels in the very first frame
while black text pixels (value 0) keep their drive-dark code. The `frame_count` parameter
was removed from `update_lut` since it was incorrect for the Fast path.

## 2026-08-12 11:30

Speed up e-paper page turns — four independent optimizations.

**Pre-allocate 64 KB waveform LUT** (`src/driver/display.rs`): moved `lut` from a
per-flush `vec!` allocation into a `Box<[u8; 65536]>` field on `Display`, initialized
once in `Display::new()`. Eliminates one 64 KB heap allocation on every `flush()` and
`flush_region()` call.

**Stack-allocate DMA row buffers** (`src/driver/display.rs`): rewrote
`prepare_dma_buffer` to write into a caller-supplied `&mut [u8; BYTES_PER_LINE]` array
instead of returning a `Vec<u8>`. The three intermediate `Vec` allocations (epd\_input,
wide\_epd\_input, line\_data\_16) are eliminated; callers now declare `let mut dma_buf =
[0u8; BYTES_PER_LINE]` on the stack. Removes up to ~16,200 heap allocations per full
page turn.

**Fast 4-frame waveforms** (`src/driver/display.rs`): added `DrawMode::Fast` and
`DrawMode::FastClear` variants backed by new 4-element timing tables
(`CONTRAST_CYCLES_FAST`, `CONTRAST_CYCLES_FAST_WHITE`). Frame count is now derived from
`contrast_cycles().len()` instead of a fixed const, so both 4-frame and 15-frame paths
share the same `draw()` / `flush_region()` code. `update_lut` takes an explicit
`frame_count` parameter. Tune `CONTRAST_CYCLES_FAST` on-device if contrast is
insufficient.

**Periodic full-quality refresh** (`examples/ereader_ui.rs`, `src/appstate.rs`): added
`AppState::full_quality_count`. Full-screen page turns use `FastClear` + `Fast` by
default; every `FULL_QUALITY_INTERVAL = 5` turns a 15-frame `WhiteOnBlack` +
`BlackOnWhite` pass runs to clear ghost accumulation. `Rgb565ToGray4::flush_with_mode()`
added to pass the chosen mode through the bridge.

## 2026-08-12 10:20

Add PSRAM and SRAM free memory display to battery dialog.

- `src/hardware.rs`: Added `MemoryInfo` struct; added `memory_info()` to `HardwareAccess` trait. `SimHardware` returns zeros; `EspHardware` reads `esp_alloc::HEAP.free_caps()` for External (PSRAM) and Internal (SRAM) regions.
- `examples/ereader_ui.rs`: Added `mem_psram` and `mem_sram` labels to battery dialog; added `fmt_bytes()` helper (formats as KB or MB); `update_battery_labels()` now populates both labels on each open.

## 2026-08-12 09:59

Replace test-subset fonts with full versions from Google Fonts / source.

- `fonts/Alegreya-{Regular,Bold,Italic}.ttf`: replaced 16 KB test subsets with full fonts (~255 KB each).
- `fonts/AtkinsonHyperlegible-{Regular,Bold,Italic,BoldItalic}.ttf`: updated to current full release; added BoldItalic variant.
- `fonts/CrimsonPro-{Regular,Bold,Italic,BoldItalic}.ttf`: replaced 13 KB test subsets with full fonts (~107 KB each); added BoldItalic.
- `fonts/Literata-{Regular,Bold,Italic}.ttf`: replaced 17 KB test subsets with full fonts (~255 KB each).
- `fonts/NoticiaText-{Regular,Bold,Italic,BoldItalic}.ttf`: updated to current release; added BoldItalic.
- `fonts/Vollkorn-{Regular,Bold,Italic}.ttf`: replaced 20 KB test subsets with full fonts (~345 KB each).

## 2026-08-12 09:58

Switch body font from NoticiaText to CrimsonText; restore real bold and italic rendering.

- `fonts/`: Added `CrimsonText-Regular.ttf`, `CrimsonText-Bold.ttf`, `CrimsonText-Italic.ttf` from Google Fonts (~105–110 KB each, compact enough to fit all three in PSRAM).
- `examples/ereader_ui.rs`: Added `BODY_FONT_BOLD_BYTES` and `BODY_FONT_ITALIC_BYTES` statics; `load_fonts()` now parses all three CrimsonText variants into separate `fontdue::Font` instances and wires them into `AppFonts.body_bold` and `AppFonts.body_italic` instead of aliasing to `body`. Bold/italic text in EPUBs now renders visually distinct from regular body text.

## 2026-08-11 16:30

Fix ESP OOM crash when loading 5 body fonts.

Root cause: each NoticiaText font uses ~2.1 MB of PSRAM in fontdue (many complex bezier curves → large `Vec<Line>` in `Geometry::finalize`). After loading 4 fonts only 2 MB remained — not enough for the 5th font's peak allocation during parsing. The 16 KB subset fonts (Alegreya etc.) have the opposite problem: missing glyphs for ebook use.

- `examples/ereader_ui.rs`: Drop `BODY_BOLD_FONT_BYTES` and `BODY_ITALIC_FONT_BYTES`; `load_fonts()` now parses only `NoticiaText-Regular` and shares the single `&'static Font` for all three body roles (`body`, `body_bold`, `body_italic`). Total PSRAM drops from ~6.4 MB to ~4.2 MB, leaving ~4 MB free. Bold/italic text renders in the regular weight until compact full-coverage bold/italic TTFs (~30–60 KB) are available. Added a one-line heap-capacity log after allocator init.

## 2026-08-11 15:45

Fix library dialog height: dialog was collapsing to just label + buttons because the embedded list uses content-based sizing (height = items × row_height), returning 0 when empty.

- `rust-embedded-gui/src/list_view.rs`: `layout_list` now respects `v_flex: Grow` — when set, the list fills the available space passed from its parent (`e.space.h`) instead of sizing to content. Shrink behavior (size = items × 2 × char_height) is unchanged.
- `examples/ereader_ui.rs`: Library dialog changed from `v_flex: Shrink, size: (440, 0)` to `v_flex: Fixed, size: (440, 400)`. A Shrink parent with a Grow child is a circular dependency (parent wants to size to fit children, child wants to fill parent); Fixed resolves this by giving the Grow list a concrete container height to fill.

## 2026-08-11

Bold and italic rendering for HTML/EPUB content.

- `src/font.rs`: Added `AppFonts` struct grouping all five font faces (`ui`, `ui_bold`, `body`, `body_bold`, `body_italic`). It is `Copy` so it can be threaded through call stacks cheaply.
- `examples/ereader_ui.rs`: Added `BODY_BOLD_FONT_BYTES` (NoticiaText-Bold) and `BODY_ITALIC_FONT_BYTES` (NoticiaText-Italic) static arrays. `load_fonts()` now returns `AppFonts` instead of a 3-tuple. `make_scene()`, `make_theme()`, and `init_app_state()` all updated to use `AppFonts`; `AppState.fonts` replaces the old `bold_font`/`body_font` fields. All `cfg_from_scene()` call sites updated.
- `src/appstate.rs`: `AppState` now holds `fonts: AppFonts` instead of individual font fields. `cfg_from_scene()` takes `&AppFonts` instead of two separate font parameters.
- `src/bookview.rs`: `BookState` now holds `fonts: AppFonts`. `layout_cfg()` takes `&AppFonts` and builds `bold_font` and `italic_font` `FontMetrics` entries for the new `LayoutConfig` fields. `next_ttf_line()` tracks `in_bold`/`in_italic` state across sentinel bytes and uses the correct font for per-character advance widths. `render_ttf_text()` draws each line in segments, flushing and switching fonts at each `\x04`–`\x07` sentinel.
- `src/layout.rs`: `LayoutConfig` gains `bold_font: Option<FontMetrics>` and `italic_font: Option<FontMetrics>`. `layout_chapter()` builds `bcache`/`icache` alongside `gcache`/`hcache`, tracks `in_bold`/`in_italic` state (reset at paragraph breaks), handles inline sentinels as zero-width style switches, and selects the right cache per character for accurate line-break widths.
- `src/epub.rs`: `apply_tag()` now emits `\x04`/`\x05` around `<b>`/`<strong>` and `\x06`/`\x07` around `<em>`/`<i>`.
- `src/book.rs`: `on_open()`/`on_close()` emit the same sentinels for HTML files.
- `src/reader.rs`: test `fixed_cfg()` updated with the new `bold_font: None, italic_font: None` fields.

Unified hold-based page navigation: regular page turns now fire on button/key release rather than press, and all nav keys share the same hold-duration logic as the fast-paging keys.

- `examples/ereader_ui.rs`:
  - **`FastPaging::end()`**: extended to handle the short-press case — if `fs_pressed_at` was set but `fs_active` is false (held < 1 s), calls `nav_prev_page` or `nav_next_page` on release, using the stored `forward` direction. Added `cancel()` method for the just-woke case (resets state without navigation).
  - **Simulator key events**: Left, Backspace, and Up all call `start_backward()` on KeyDown; Right, Space, and Down all call `start_forward()` on KeyDown. KeyUp for each group calls `fast.end()`, which now handles both fast-paging navigation and regular single-page turns. Removed the old immediate-nav-on-KeyDown handlers for Left/Backspace/Right/Space.
  - **ESP button loop**: `start_forward/backward` is now called once before the hold loop (only when not `just_woke`), fixing a bug where calling it every 10 ms iteration reset `fs_pressed_at` and prevented the 1 s threshold from ever being reached (fast paging never triggered on hardware). After the loop, replaced inline `if fast.fs_active / else if just_woke / else nav` block with `fast.cancel()` (just-woke path) or `fast.end()` (normal path).

## 2026-08-10

Battery info dialog: tapping the battery percentage in the top bar now opens a dialog showing charge percentage, voltage, and charging status, with a Dismiss button to close it.

- `src/hardware.rs`: Added `BatteryInfo` struct (`percent`, `voltage_mv`, `is_charging`) and `battery_info()` method to the `HardwareAccess` trait. Both `SimHardware` and `EspHardware` return stub values (85%, 4050 mV, not charging) as placeholder until real ADC reading is wired up.
- `examples/ereader_ui.rs`: Added `BATTERY_BUTTON_ID`, `BATTERY_DIALOG_ID`, `BATTERY_CLOSE_ID` constants. Changed the battery top-bar label to a `make_button` so it receives tap events. Added a centered `battery_dialog` panel with percent/voltage/status labels and a Dismiss button. Added `update_battery_labels()` helper that refreshes dialog labels from `hw.battery_info()` on each open. Wired click handlers for show/hide.

## 2026-08-08

Two-tier inactivity sleep: light sleep at 60 s, deep sleep at 60 min.

- `src/hardware.rs`:
  - Added `enter_light_sleep(&mut self)` to the `HardwareAccess` trait and the simulator default (no-op).
  - ESP impl: turns off backlight, enables `GpioWakeupSource` on both GPIO0 and GPIO38 via `wakeup_enable(true, WakeEvent::LowLevel)`, calls `sleep_light`, and restores backlight on return. GPIO38 can only wake from light sleep (not an RTC pin), so deep sleep still uses `Ext0WakeupSource` on GPIO0 only.
  - Added `GpioWakeupSource` and `WakeEvent` to ESP imports.
- `examples/ereader_ui.rs`:
  - Replaced `SLEEP_AFTER_SECS = 60` with `LIGHT_SLEEP_AFTER_SECS = 60` and `DEEP_SLEEP_AFTER_SECS = 3600`.
  - Added `just_woke: bool` flag. After returning from light sleep, the first button press is consumed as a wake event (backlight restore only, no page turn), then `just_woke` is cleared.
  - The idle check is now a two-branch `if/else if`: `>= DEEP_SLEEP_AFTER_SECS` triggers the existing deep sleep path; `>= LIGHT_SLEEP_AFTER_SECS` calls `enter_light_sleep()`.

## 2026-08-07 (10)

Restore the last-read book when waking from deep sleep.

- `src/hardware.rs`: Added `save_last_filename(filename: &str)` and `load_last_filename() -> Option<String>` (ESP-only). The filename is packed as 4-byte LE chunks into NVS keys 50 (length) and 51–66 (data), giving up to 64 bytes. Sentinel filenames (starting with `__`) store length=0 so the welcome screen is shown on wakeup when no SD book was open.
- `examples/ereader_ui.rs`:
  - Before entering deep sleep, `save_last_filename(&current_filename)` is now called alongside the existing bookmark save.
  - On sleep wakeup (`is_sleep_wakeup`), `load_last_filename()` is called before the event loop. If a filename is found, the loading dialog is shown and force-flushed to e-paper, the SD file is loaded, and the book/session are set up using the chapter and anchor already in the RTC registers. If the SD card is absent or the file is missing, the device falls back silently to the welcome HTML.

## 2026-08-07 (9)

Show a "Loading…" dialog before the blocking book-file read so the user gets feedback immediately.

- `examples/ereader_ui.rs`: Added `LOADING_DIALOG_ID` constant, `show_loading_dialog` and `hide_loading_dialog` helpers, and a centered 320 px-wide loading panel in `make_scene`.
- In both the simulator and ESP `library_read` action arms: the library dialog is now hidden and the loading dialog shown *before* calling `hw.load_book_file`. An inline force-flush (layout + draw + display flush) writes the "Loading \<filename\>…" message to the screen before the blocking I/O begins. On the ESP, the e-paper panel remains visible for the entire duration of the multi-second SD card read without needing a background thread (e-paper is bistable). After the load succeeds the loading dialog is hidden and the book shown; on failure it is hidden and the error dialog shown.

## 2026-08-07 (8)

Make the library dialog narrower so partial refresh covers less of the screen.

- `rust-embedded-gui/src/layouts.rs`: Fixed a bug in `layout_vbox` where Grow children were laid out with the parent's full available space width (`pass.space.w`) instead of the panel's own content width (`available_space.w`). For Fixed-width panels this caused Grow children (e.g. the list view) to size themselves wider than the panel, potentially overflowing the framebuffer.
- `rust-embedded-gui/src/scene.rs`: Added `mark_layout_dirty_view(name)` — sets `layout_dirty = true` and marks only the named view's bounds dirty, without expanding the dirty rect to the full screen as `mark_layout_dirty()` does.
- `examples/ereader_ui.rs`:
  - Library dialog panel now uses `layout_centered_dialog` with `Flex::Fixed` width (440 px) and `Flex::Grow` height. In portrait mode this reduces the physical row count from 540 to 440 (≈19% fewer row writes per refresh frame).
  - `handle_click` for library open/close no longer calls `mark_dirty_all()` or `mark_layout_dirty()`. `show_view` / `hide_view` already mark just the dialog area dirty; the extra full-screen marks were causing unnecessary full-screen refreshes.
  - Simulator action handler for the "library" source now calls `mark_layout_dirty_view(&LIBRARY_DIALOG_ID)` instead of `mark_layout_dirty() + mark_dirty_all()`, and list-selection changes call `mark_dirty_view(&LIBRARY_DIALOG_ID)` instead of `mark_dirty_all()`.

## 2026-08-07 (7)

Fix overdraw outside dialog during partial refresh.

- `src/driver/display.rs`: `flush_region()` was calling `self.epd.skip()` (raw CKV-only pulse) for rows outside the dirty rectangle. This left the display's output latch holding whatever waveform the last in-range row drove; with OE active, every skipped row was driven by that stale latch. The clearing pass (WhiteOnBlack) latch bled into rows after the dialog (left side in portrait → went white) and the prior full-flush book-content latch bled into rows before the dialog (right side → went black). Fix: replace `epd.skip()` with `self.row_skip()` for out-of-range rows. `row_skip()` explicitly writes VCOM for the first two skipped rows to neutralize the latch, then falls back to CKV-only — the same mechanism `draw()` already uses for untainted rows. As a secondary effect, `self.skipping` is now non-zero after out-of-range rows, so the end-of-frame guard no longer fires an extra `row_write()` that was driving the first row outside the dialog with the last in-dialog row's waveform.

## 2026-08-07 (6)

Render HTML/EPUB headings in bold at a larger size.

- `src/book.rs`: `on_open` pushes a sentinel byte (`\x01`=H1, `\x02`=H2, `\x03`=H3+) immediately after the `\n\n` that opens a heading paragraph.
- `src/epub.rs`: `apply_tag` does the same for EPUB XHTML heading tags on the opening tag only.
- `src/layout.rs`: `LayoutConfig` gains `heading_font: Option<FontMetrics>`. `layout_chapter` detects sentinel bytes, switches to heading metrics (larger line height + measure fn) for the rest of that paragraph, and resets on the next `\n\n`.
- `src/bookview.rs`: `BookState` gains `heading_font` and `heading_font_px`. `layout_cfg` accepts a `heading_font` parameter and builds the `FontMetrics` at 1.4× body size. `render_ttf_text` detects sentinels at paragraph boundaries and switches font/size for heading paragraphs, stripping the sentinel before drawing.
- `examples/ereader_ui.rs`: `make_scene` and `cfg_from_scene` accept `bold_font`; `BookState` is initialised with `heading_font = bold_font` (AtkinsonHyperlegible-Bold) at 1.4× body size.

## 2026-08-07 (5)

Replace default Sherlock Holmes EPUB with a built-in welcome guide.

- `examples/welcome.html`: New HTML welcome page explaining buttons, library, settings, screen cleaning, and bookmark saving.
- `examples/ereader_ui.rs`: Replaced `EPUB_DATA` / `sherlock_holmes.epub` with `WELCOME_HTML` / `welcome.html`. Both the simulator and ESP paths now open the welcome guide on a fresh start (`current_filename = "__welcome__"`), so the Sherlock Holmes text is no longer embedded in the firmware.

## 2026-08-07 (4)

Add `deep_clean()` to `Display` and a "Clean Screen" button in the settings dialog.

- `src/driver/display.rs`: Added `deep_clean(cycles: u8)` — runs `cycles` alternating fill-black + `BlackOnWhite` flush + fill-white + `WhiteOnBlack` flush passes to discharge residual gate-line charge imbalance that builds up as faint lines over repeated partial refreshes.
- `examples/ereader_ui.rs`: Added `DEEP_CLEAN_ID` constant and a "Clean Screen" button to the settings dialog. In the ESP handler it calls `display.deep_clean(3)` and resets `partial_refresh_count`. Simulator handler is a no-op.

## 2026-08-07 (3)

Periodic full refresh to prevent gradual darkening of white areas outside the dirty rect.

- `examples/ereader_ui.rs`: Added `partial_refresh_count` counter and `PARTIAL_REFRESH_FULL_INTERVAL = 8` constant before the main loop. Both the fast-scroll and main-loop partial refresh blocks now increment the counter on each partial refresh and force a full `fill(0x0F)` + `flush(WhiteOnBlack)` + full-screen draw every 8 partial refreshes (then reset the counter). A full-screen refresh always resets the counter to 0.

**Root cause:** Repeated partial waveform passes cause capacitive field coupling that gradually darkens e-paper pixels adjacent to the dirty column/row boundaries. A periodic full refresh resets all pixels to a clean state. The interval of 8 means ghosting accumulates for at most a few selection changes before the display is fully reset.

## 2026-08-07 (2)

Fix partial refresh: previously selected list items stay black after deselection.

- `src/driver/display.rs`: Added `fill_region(area, color)` (utility) and `flush_region(area, mode)`. `flush_region` drives the waveform only within the physical rectangle: rows outside get fast CKV skips; for rows inside, `mask_dma_columns` zeros out the 2-bit waveform values for columns outside the x-range (VCOM = no drive), so content outside the column range is physically untouched.
- `examples/ereader_ui.rs`: Added `clearing_flush_region(dirty_rect)` to `Rgb565ToGray4` — converts the logical `Bounds` to a physical `Rectangle` (samples 4 corners to handle all orientations) and calls `display.flush_region(WhiteOnBlack)`.
- `examples/ereader_ui.rs`: Both ESP partial refresh blocks now call `clearing_flush_region` for the ghost-clear pass instead of a bare `fill(0x0F)` + `flush(WhiteOnBlack)`. Full-screen refreshes (`needs_full_refresh`) still use the original `fill(0x0F)` + `flush(WhiteOnBlack)` path unchanged.

**Root cause:** After each `flush()`, the framebuffer resets to 0xFF. The `BlackOnWhite` LUT immediately clears all drive bits for white-target pixels — so nothing drives physically-black pixels to white. A preceding `WhiteOnBlack` clearing pass is needed; without column masking it was incorrectly driving full row widths to white, wiping content outside the dirty area.

## 2026-08-07

Partial refresh: limit e-paper waveform to dirty row range only.

- `src/driver/display.rs`: `draw()` now computes the first and last tainted physical rows before rendering. Rows outside that range receive a bare `epd.skip()` (fast CKV pulse, no pixel data) rather than going through `row_skip()`. For landscape partial updates this reduces the scan to just the dirty rows; for portrait mode it also narrows the scan to the logical-x → physical-y footprint of the changed area, avoiding full-panel waveform passes for small updates.

## 2026-08-06 (6)

Add truncating book-title label to the top bar.

- `examples/ereader_ui.rs`: Added `make_truncating_label` helper function. It uses `h_flex: Grow` so the hbox allocates it a fixed share of remaining space. The draw closure measures the title against available width; if it fits, draws it normally; if not, walks characters until the text plus "..." fits and draws the truncated form. The "booktitle" label in the top bar now uses this variant.

## 2026-08-06 (5)

Add `PARA_GAP_LINES` constant to control paragraph spacing. Set to `2` (two line-heights of extra space between paragraphs).

- `src/layout.rs`: new `pub const PARA_GAP_LINES: u32 = 2` used as the multiplier for the paragraph gap in `layout_chapter`.
- `src/bookview.rs`: `render_ttf_text` uses the same constant so layout and render stay in sync.

## 2026-08-06 (4)

Fix page rendering clipping caused by double-newline paragraph break mismatch.

- `src/bookview.rs`: `render_ttf_text` now handles `\n\n` consistently with `layout_chapter`. Previously each `\n` caused a full `line_h` baseline advance (totalling 2×`line_h` for `\n\n`), while `layout_chapter` uses `line_h + para_gap = 1.5×line_h`. Each paragraph break consumed an extra 0.5×`line_h` in the renderer, causing the last lines of a page to be clipped. Fix: when the rest-of-text after a line starts with `\n`, consume it and advance only `para_gap = line_h/2` instead of `line_h`.

## 2026-08-06 (3)

Fix content layout height bug: text was being clipped because `layout_cfg` used hardcoded bar-height estimates instead of actual content view dimensions.

- `src/bookview.rs`: `layout_cfg` now accepts `content_w`/`content_h` (actual content view pixel dimensions) and subtracts only the renderer's fixed padding (32 px horizontal, 24 px vertical). Removed the old hardcoded chrome/bar height estimation.
- `examples/ereader_ui.rs`: Added `cfg_from_scene` helper that runs `layout_scene` to get real content view bounds before calling `layout_cfg`. All call sites (simulator and ESP paths) updated to use `cfg_from_scene`. Two regression tests added and now pass: `layout_cfg_height_matches_content_view` and `page_line_count_fits_in_content_view`.

## 2026-08-06 (2)

Fast page-scroll overlay: hold Up/Down (simulator) or Prev/Next buttons (ESP) for >1 s.

- `examples/ereader_ui.rs`: Added `FAST_SCROLL_PANEL_ID` constant, `layout_fast_scroll_panel` (centers a 300×80 panel), and `update_fast_scroll_label` helper. A small centered panel appears after a 1-second hold showing "Ch N/Total · Pg N/Total"; the counter advances every 200 ms. At chapter boundaries the session loads the adjacent chapter so the scroll continues across chapters. Only the panel area is re-rendered during the hold (partial refresh). On release the book jumps to the target chapter/page and the panel hides. Short press still does a normal single-page turn (ESP). Simulator uses Up/Down arrow keys; ESP restructures the button polling loop to detect hold duration.

## 2026-08-06

Library dialog now requires an explicit "Read" button tap to open a book.

- `examples/ereader_ui.rs`: Added a "Read" button alongside "Cancel" in the library dialog footer row. Tapping a list item now only highlights it; tapping "Read" loads the selected book. Updated both the simulator and hardware event loops.

## 2026-08-05 (6)

Show LFN (long file names) on SD card; filter Apple Double files.

- `src/hardware.rs`: `list_book_files` and `load_book_file` now use `iterate_dir_lfn` with a 256-byte `LfnBuffer` so the full long filename is shown instead of the 8.3 mangled SFN. Apple Double files (macOS metadata) are filtered: with LFN they start with `._`, without LFN their SFN starts with `_` after 8.3 mangling.

## 2026-08-05 (5)

Per-book bookmark table: each book remembers its reading position independently.

- `src/hardware.rs`: Replaced single-slot `save_position`/`load_position` with an 8-slot bookmark table keyed by FNV-1a filename hash. `save_bookmark(filename, chapter, anchor)` updates an existing slot, fills an empty one, or evicts slot 0 when the table is full. `load_bookmark(filename)` returns `None` on missing entry or any flash error — never panics. Simulator implementation is a no-op returning `None`. Added `load_cold_boot_position()` free function for the ESP cold-boot path (reads the `"__embedded__"` slot, defaults to (0,0)).
- `examples/ereader_ui.rs`: Both sim and ESP loops now track `current_filename`. Page-turn and deep-sleep paths call `save_bookmark`. Library switch saves the old book's position then tries `load_bookmark` for the new book, using `BookSession::restore` if a position exists and `BookSession::new` otherwise.

## 2026-08-05 (4)

Handle non-UTF-8 files and book load failures gracefully.

- `src/book.rs`: Added `decode_lossy` — tries UTF-8, falls back to Latin-1 (ISO-8859-1) so Windows-1252 encoded HTML/TXT files display instead of crashing. `TxtBook` and `HtmlBook` both use it.
- `examples/ereader_ui.rs`: Added error dialog (`ERROR_DIALOG_ID`) with a Dismiss button. `show_error_dialog` sets the filename as the message. Both book-load failure sites (sim and ESP loops) now show the dialog instead of panicking or silently warning.

## 2026-08-05 (3)

Fix SD card not detected on ESP hardware.

- `src/hardware.rs`: Deselect LoRa CS (GPIO 46) before initializing the SD SPI bus — SD and LoRa share SPI2, and a floating LoRa CS was driving MISO and preventing the SD card from responding.
- `src/hardware.rs`: Set SPI clock to 400 kHz (SD init spec max) instead of the default.
- `src/hardware.rs`: Send 80 clock pulses on the raw `SpiBus` with CS high before creating `ExclusiveDevice`, satisfying the SD card's 74-cycle power-up requirement that `ExclusiveDevice` would otherwise skip.

## 2026-08-05 (2)

Fix ESP and simulator compile errors in `examples/ereader_ui.rs`.

- All `make_label("id", ...)` calls updated to `make_label(&ViewId::new("id"), ...)` to match the current `&ViewId` signature (affected 11 call sites).
- `src/book.rs`: `s.to_owned()` replaced with `String::from(s)` — `ToOwned` is not in scope in the `alloc`/no_std environment.
- `examples/ereader_ui.rs`: Added `vec::Vec` to the ESP `alloc` import; removed unused imports (`char_advance`, `draw_str`, `line_height`, `measure_width`, `FontMetrics`, `Size`, `layout_std_panel`, `DrawEvent`, `util`); removed spurious `mut` on `scene` and `hw` parameters in `nav_next_page`/`nav_prev_page`.

## 2026-08-05

Add tests for HTML/text parsing and reader pagination.

- `src/book.rs`: 27 tests covering `TxtBook` (spine, content, invalid UTF-8), `HtmlBook`, and `html_to_text` (whitespace collapse, skipped sections, block elements, named/numeric entities, case-insensitive and self-closing tags).
- `src/reader.rs`: 18 tests covering `ReaderState` (page count, navigation, boundary clamping, `relayout` anchor preservation) and `BookSession` (open, chapter navigation, `restore`), using a `MockBook` helper for multi-chapter scenarios.

## 2026-08-04 18:00

Add support for reading `.html` and `.txt` files.

- New `src/book.rs`: `Book` trait (`spine`, `chapter_text`), `TxtBook` (whole file as one chapter), `HtmlBook` (HTML-to-text converter preserving headings, paragraphs, lists, and common entities).
- `src/epub.rs`: `impl Book for EpubArchive` delegating to existing methods.
- `src/reader.rs`: `BookSession` methods now take `&dyn Book` instead of `&EpubArchive`.
- `src/hardware.rs`: Renamed `list_epub_files`/`load_epub_file` → `list_book_files`/`load_book_file`. Simulator filter expanded to `.epub`, `.html`, `.htm`, `.txt`. ESP SD card filter expanded to include `.HTM` and `.TXT` short extensions.
- `examples/ereader_ui.rs`: `epub: EpubArchive` replaced by `book: Box<dyn Book>` in both main loops. New `book_from_data(filename, data)` helper dispatches on file extension to construct the correct type. Library dialog now lists all supported formats.
- `library/sample.html` and `library/sample.txt` added for simulator testing.

## 2026-08-04 17:00

Consolidate dialog ViewId string literals to constants; rename library Cancel button.

- `examples/ereader_ui.rs`: Replace inline `ViewId::new("dialog")` and `ViewId::new("library_dialog")` with the existing `DIALOG_ID` and `LIBRARY_DIALOG_ID` constants throughout `handle_click` and both event loops.
- Rename library dialog "Close" button label to "Cancel".

## 2026-08-04 16:30

Fix crashes in library dialog list view.

- `rust-embedded-gui/src/list_view.rs`: Guard against divide-by-zero and integer overflow when `items` is empty or `cell_height` is zero (dialog hidden during initial layout gives zero bounds). Added early-return guards in `draw_list` and `input_list`, and usize underflow guard in `select_next`.
- `examples/ereader_ui.rs`: Call `scene.mark_layout_dirty()` when opening the library dialog and populating its list, so the list view's height is recomputed from item count before the next draw.

## 2026-08-04 16:00

Add Library feature: list and load epub files at runtime.

A new "Library" button in the topbar opens a dialog listing available epub files. Selecting a file loads it, replacing the current book. On the simulator, files are read from `examples/library/`. On ESP, files are read from the SD card root.

Changes:
- `src/epub.rs`: Removed lifetime from `EpubArchive` — backing store is now owned `Vec<u8>`. Kept `new(&[u8])` for backward compatibility (copies bytes). Added `from_vec(Vec<u8>)` zero-copy constructor for runtime loading.
- `src/hardware.rs`: Added `list_epub_files() -> Vec<String>` and `load_epub_file(name) -> Option<Vec<u8>>` to the `HardwareAccess` trait. `SimHardware` reads from `./library/` via `std::fs`. `EspHardware` steals SPI2 + GPIO 12/13/14/21 on demand to access the SD card via `embedded-sdmmc`.
- `Cargo.toml`: Added `embedded-sdmmc = "0.9"` and `embedded-hal-bus = "0.2"` as optional deps under the `esp` feature.
- `examples/ereader_ui.rs`: Library button and dialog added to scene. Dialog uses `make_list_view` populated lazily when opened. Both simulator and ESP event loops handle `"library"` (open/populate), `"library_close"` (close), and `"lib_list"` (select and load) events. `EpubArchive` is now mutable in both loops to allow reassignment.
- `examples/library/`: Created directory with `moby_dick.epub` and `sherlock_holmes.epub` for simulator testing.

## 2026-08-04 14:30

Replace subsetted AtkinsonHyperlegible fonts with full versions from Google Fonts.

The bundled `AtkinsonHyperlegible-Regular.ttf` and `AtkinsonHyperlegible-Bold.ttf`
were 15 KB subsets sharing an identical 60-glyph character set that omitted 'M',
'D', 'E', 'J', 'K', 'U', 'W', 'X', 'Z', '?', '-', ':', '<', '>', and '%'. This
caused missing glyphs on UI labels such as "Medium", "< Prev", "Next >", "--:-- --",
and "85%". Replaced both files with the full v12 fonts (51–52 KB) downloaded from
Google Fonts (fonts.gstatic.com). No code changes required.

## 2026-08-04 14:00

Move flash persistence behind `HardwareAccess` trait; add `save_position` and `save_settings`.

Previously the ESP main function in `ereader_ui.rs` contained ~140 lines of
hardware-specific flash storage code (`FlashAdapter`, `block_on`, NVS constants,
`save_position`, `save_settings`, `load_settings`, `load_position`) that had no
simulator equivalent and cluttered the example.

Changes:
- `src/hardware.rs`: Added two new trait methods to `HardwareAccess`:
  - `save_position(&mut self, chapter_idx, anchor_byte)` — writes chapter/anchor to NVS flash on ESP, no-op on simulator.
  - `save_settings(&mut self)` — writes font/backlight/orientation from `self` to NVS flash on ESP, no-op on simulator.
  Moved all flash storage infrastructure (`FlashAdapter`, `block_on`, NVS key constants) into `hardware.rs`. `load_settings()` and `load_position()` remain as `pub` standalone functions since they must be called before `EspHardware` is constructed (chicken-and-egg with initialization).
- `examples/ereader_ui.rs`: Removed all duplicated flash code. Updated 7 call sites: `save_position(...)` → `hw.save_position(...)`, `save_settings(hw.font_size()..., ...)` → `hw.save_settings()`. Added `load_settings`/`load_position` to the ESP import line.

## 2026-08-03 16:00

Fix settings dialog not reflecting persisted font/backlight/orientation after reboot.

The three toggle groups in the settings dialog were created with hardcoded defaults
(Medium / High / Portrait), so after a cold boot the UI always showed those defaults
even though the correct values had been loaded from NVS flash. A user seeing the
wrong selection highlighted would click it to "confirm", inadvertently overwriting
their saved preference.

Added `sync_settings_ui()` which updates the `SelectOneOfState.selected` field of
each toggle group to match the loaded indices. Called from the ESP main function
immediately after `make_scene()`, before the first render.

## 2026-08-03 15:15

Fixed OOM crash during glyph rasterization on device.

Root cause: `fontdue::Font::from_bytes()` was being called in three separate places
(theme fonts via `load_fonts()`, layout measurement in `layout_cfg`, and book
rendering in `BookState`), creating up to four Font instances simultaneously. Each
parsed Font occupies tens of KB on the heap; together they exhausted SRAM (72 KB)
before a rasterization `Vec<Line>` could grow.

Changes:
- `src/font.rs`: `TextRenderer` now holds `&'static fontdue::Font` instead of an
  owned `Font`. `TextRenderer::from_static(font)` is the new preferred constructor
  (zero allocation). `new()` and `with_font()` are kept but now leak via `Box::leak`
  for use in standalone examples.
- `examples/ereader_ui.rs`: `load_fonts()` is called first in both main functions;
  the resulting `&'static Font` is threaded through `make_scene` and `layout_cfg` so
  all TextRenderers share the same two underlying Font objects (Regular + Bold).

## 2026-08-03 14:30

Added `ENABLE_WIFI_NTP` constant to disable the WiFi init and NTP time sync.

- New `const ENABLE_WIFI_NTP: bool` (ESP-only) declared alongside `SSID`/`PASSWORD`.
- Set to `false` to skip WiFi stack initialization and NTP query entirely (e.g. when
  no network is present or to speed up boot during development).
- Set to `true` (default) to retain existing cold-boot NTP sync behavior.

## 2026-08-03 (Load theme TTF fonts via Box::leak — works on both simulator and ESP)

Replaced the previous cfg-gated `get_ttf_font()` approaches (OnceLock on simulator,
unsafe MaybeUninit on ESP) with a single unified pattern:

- `FONT_BYTES` / `BOLD_FONT_BYTES` — `include_bytes!` statics that embed
  `fonts/NoticiaText-Regular.ttf` and `fonts/NoticiaText-Bold.ttf` at compile time.
  No filesystem access at runtime; works identically on simulator and ESP.
- `load_fonts()` — parses both fonts and calls `Box::leak(Box::new(font))` to
  obtain `&'static fontdue::Font` references. `Box` is available from `std` on the
  simulator and from `alloc` on ESP, so no `#[cfg]` is needed.
- `make_theme(font, bold_font)` now takes the two static font references directly
  instead of calling a platform-specific getter internally.

## 2026-08-03 (Fix f32::round() unavailable in no_std ESP build)

`f32::round()` requires the std math library (or libm) and is not available in `no_std`.
Replaced all three `.advance_width.round() as i32` calls in `iris-ui/src/device.rs`
with `(advance_width + 0.5) as i32`, which rounds positive f32 values identically
without any additional dependency. Advance widths from fontdue are always positive so
the equivalence holds exactly.

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
