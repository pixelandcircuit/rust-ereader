/// Hardware abstraction for the e-reader device.
///
/// Provides uniform access to font size, backlight level, orientation,
/// and current time across the simulator and the real ESP32-S3 device.

#[cfg(feature = "esp")]
extern crate alloc;

#[cfg(feature = "esp")]
use alloc::string::String;
#[cfg(feature = "esp")]
use alloc::vec::Vec;
#[cfg(not(feature = "esp"))]
use std::string::String;
#[cfg(not(feature = "esp"))]
use std::vec::Vec;

// Physical panel dimensions (landscape orientation).
pub const PANEL_W: u16 = 960;
pub const PANEL_H: u16 = 540;

// ── Types ─────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum FontSize {
    Small,
    Medium,
    Large,
}

impl FontSize {
    pub fn from_cmd(cmd: &str) -> Self {
        match cmd {
            "Small" => Self::Small,
            "Large" => Self::Large,
            _ => Self::Medium,
        }
    }

    pub fn to_index(self) -> usize {
        match self {
            Self::Small => 0,
            Self::Medium => 1,
            Self::Large => 2,
        }
    }

    pub fn from_index(i: usize) -> Self {
        match i {
            0 => Self::Small,
            2 => Self::Large,
            _ => Self::Medium,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum BacklightLevel {
    Off,
    Low,
    High,
}

impl BacklightLevel {
    pub fn from_cmd(cmd: &str) -> Self {
        match cmd {
            "Off" => Self::Off,
            "Low" => Self::Low,
            _ => Self::High,
        }
    }

    pub fn to_index(self) -> usize {
        match self {
            Self::Off => 0,
            Self::Low => 1,
            Self::High => 2,
        }
    }

    pub fn from_index(i: usize) -> Self {
        match i {
            0 => Self::Off,
            1 => Self::Low,
            _ => Self::High,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Orientation {
    Portrait,
    Landscape,
    ReversePortrait,
    ReverseLandscape,
}

impl Orientation {
    pub fn from_cmd(cmd: &str) -> Self {
        match cmd {
            "Land" => Self::Landscape,
            "R.Port" => Self::ReversePortrait,
            "R.Land" => Self::ReverseLandscape,
            _ => Self::Portrait,
        }
    }

    pub fn to_index(self) -> usize {
        match self {
            Self::Portrait => 0,
            Self::Landscape => 1,
            Self::ReversePortrait => 2,
            Self::ReverseLandscape => 3,
        }
    }

    pub fn from_index(i: usize) -> Self {
        match i {
            1 => Self::Landscape,
            2 => Self::ReversePortrait,
            3 => Self::ReverseLandscape,
            _ => Self::Portrait,
        }
    }

    pub fn is_portrait(self) -> bool {
        matches!(self, Self::Portrait | Self::ReversePortrait)
    }

    /// Returns (logical_width, logical_height) for this orientation.
    pub fn logical_size(self) -> (i32, i32) {
        if self.is_portrait() {
            (PANEL_H as i32, PANEL_W as i32) // 540 × 960
        } else {
            (PANEL_W as i32, PANEL_H as i32) // 960 × 540
        }
    }

    /// Convert physical touch coordinates to logical screen coordinates.
    pub fn phys_to_logical(self, tx: u16, ty: u16) -> (i32, i32) {
        let (tx, ty) = (tx as i32, ty as i32);
        let w = PANEL_W as i32;
        let h = PANEL_H as i32;
        match self {
            Self::Portrait => (h - 1 - ty, tx),
            Self::Landscape => (tx, ty),
            Self::ReversePortrait => (ty, w - 1 - tx),
            Self::ReverseLandscape => (w - 1 - tx, h - 1 - ty),
        }
    }

    /// Convert logical pixel coordinates to physical display coordinates.
    pub fn logical_to_phys(self, lx: u16, ly: u16) -> (u16, u16) {
        let w = PANEL_W;
        let h = PANEL_H;
        match self {
            Self::Portrait => (ly, h - 1 - lx),
            Self::Landscape => (lx, ly),
            Self::ReversePortrait => (w - 1 - ly, lx),
            Self::ReverseLandscape => (w - 1 - lx, h - 1 - ly),
        }
    }
}

// ── Battery ───────────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct BatteryInfo {
    pub percent: u8,
    pub voltage_mv: u32,
    pub is_charging: bool,
}

pub struct MemoryInfo {
    pub psram_free_bytes: usize,
    pub psram_total_bytes: usize,
    pub sram_free_bytes: usize,
    pub sram_total_bytes: usize,
}

// ── Trait ─────────────────────────────────────────────────────────────────────

pub trait HardwareAccess {
    fn font_size(&self) -> FontSize;
    fn backlight_level(&self) -> BacklightLevel;
    fn orientation(&self) -> Orientation;
    fn current_time_secs(&self) -> u64;
    fn battery_info(&self) -> BatteryInfo;
    fn memory_info(&self) -> MemoryInfo;

    fn set_font_size(&mut self, size: FontSize);
    fn set_backlight_level(&mut self, level: BacklightLevel);
    fn set_orientation(&mut self, orientation: Orientation);
    /// Set the hardware RTC to the given Unix timestamp. No-op on simulator.
    fn set_current_time_secs(&mut self, secs: u64);
    /// UTC offset in minutes for local time display (e.g. -480 for UTC-8, 330 for UTC+5:30).
    fn utc_offset_minutes(&self) -> i32;
    fn set_utc_offset_minutes(&mut self, minutes: i32);

    /// Physical BOOT button (GPIO0) state — always false on simulator.
    fn button_prev_pressed(&self) -> bool;
    /// Physical NEXT button (GPIO38) state — always false on simulator.
    fn button_next_pressed(&self) -> bool;
    /// Save reading position and settings to RTC fast memory, turn off
    /// backlight, and enter deep sleep (wakes on BOOT button / GPIO0 LOW).
    /// On ESP this never returns. On simulator it is a no-op.
    fn enter_deep_sleep(&mut self, chapter_idx: usize, anchor_byte: usize);
    /// Turn off backlight, light-sleep until either button is pressed, then
    /// restore backlight and return. On simulator this is a no-op.
    fn enter_light_sleep(&mut self);

    /// Persist per-book reading position identified by `filename`.
    /// Stores up to 8 books; evicts the oldest when the table is full.
    /// No-op on simulator.
    fn save_bookmark(&mut self, filename: &str, chapter_idx: usize, anchor_byte: usize);
    /// Retrieve the saved position for `filename`. Returns `None` when no
    /// entry exists or on any flash error — never panics.
    fn load_bookmark(&self, filename: &str) -> Option<(usize, usize)>;
    /// Persist the current font/backlight/orientation settings to flash.
    /// No-op on simulator.
    fn save_settings(&mut self);
    /// Persist the last-read filename so it can be restored after sleep.
    /// No-op on simulator.
    fn save_last_filename(&mut self, filename: &str);

    /// Return bare filenames of readable files (`.epub`, `.html`, `.htm`, `.txt`).
    /// Simulator: reads `./library/`. ESP: reads SD card root (empty if no card).
    fn list_book_files(&self) -> Vec<String>;
    /// Read a book file into memory. `name` is a value from `list_book_files`.
    /// Returns `None` if the file cannot be read.
    fn load_book_file(&self, name: &str) -> Option<Vec<u8>>;
}

// ── Simulator implementation ──────────────────────────────────────────────────

#[cfg(feature = "simulator")]
pub struct SimHardware {
    font_size: FontSize,
    backlight: BacklightLevel,
    orientation: Orientation,
    utc_offset_minutes: i32,
    bookmarks: std::collections::HashMap<String, (usize, usize)>,
}

#[cfg(feature = "simulator")]
impl SimHardware {
    pub fn new() -> Self {
        Self {
            font_size: FontSize::Medium,
            backlight: BacklightLevel::High,
            orientation: Orientation::Portrait,
            utc_offset_minutes: 0,
            bookmarks: std::collections::HashMap::new(),
        }
    }
}

#[cfg(feature = "simulator")]
impl HardwareAccess for SimHardware {
    fn font_size(&self) -> FontSize {
        self.font_size
    }
    fn backlight_level(&self) -> BacklightLevel {
        self.backlight
    }
    fn orientation(&self) -> Orientation {
        self.orientation
    }

    fn current_time_secs(&self) -> u64 {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }

    fn set_font_size(&mut self, size: FontSize) {
        self.font_size = size;
    }
    fn set_backlight_level(&mut self, level: BacklightLevel) {
        self.backlight = level;
        info!("Set backlight level to {:?}", self.backlight);
    }
    fn set_orientation(&mut self, orientation: Orientation) {
        self.orientation = orientation;
    }
    fn set_current_time_secs(&mut self, _secs: u64) {} // simulator reads from system clock
    fn utc_offset_minutes(&self) -> i32 {
        self.utc_offset_minutes
    }
    fn set_utc_offset_minutes(&mut self, minutes: i32) {
        self.utc_offset_minutes = minutes;
    }

    fn button_prev_pressed(&self) -> bool {
        false
    }
    fn button_next_pressed(&self) -> bool {
        false
    }
    fn enter_deep_sleep(&mut self, _chapter_idx: usize, _anchor_byte: usize) {}
    fn enter_light_sleep(&mut self) {}
    fn save_bookmark(&mut self, filename: &str, chapter_idx: usize, anchor_byte: usize) {
        self.bookmarks
            .insert(String::from(filename), (chapter_idx, anchor_byte));
    }
    fn load_bookmark(&self, filename: &str) -> Option<(usize, usize)> {
        self.bookmarks.get(filename).copied()
    }
    fn save_settings(&mut self) {}
    fn save_last_filename(&mut self, _filename: &str) {}

    fn list_book_files(&self) -> Vec<String> {
        std::fs::read_dir("library")
            .into_iter()
            .flatten()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| {
                let l = n.to_lowercase();
                l.ends_with(".epub")
                    || l.ends_with(".html")
                    || l.ends_with(".htm")
                    || l.ends_with(".txt")
            })
            .collect()
    }

    fn load_book_file(&self, name: &str) -> Option<Vec<u8>> {
        std::fs::read(format!("library/{name}")).ok()
    }

    fn battery_info(&self) -> BatteryInfo {
        BatteryInfo {
            percent: 85,
            voltage_mv: 4050,
            is_charging: false,
        }
    }

    fn memory_info(&self) -> MemoryInfo {
        MemoryInfo { psram_free_bytes: 0, psram_total_bytes: 0, sram_free_bytes: 0, sram_total_bytes: 0 }
    }
}

// ── ESP flash storage ─────────────────────────────────────────────────────────

#[cfg(feature = "esp")]
use esp_storage::FlashStorage;
#[cfg(feature = "esp")]
use sequential_storage::{cache::NoCache, map};

#[cfg(feature = "esp")]
const NVS_RANGE: core::ops::Range<u32> = 0x9000..0xF000;
#[cfg(feature = "esp")]
const KEY_FONT: u8 = 10;
#[cfg(feature = "esp")]
const KEY_BL: u8 = 11;
#[cfg(feature = "esp")]
const KEY_ORI: u8 = 12;
#[cfg(feature = "esp")]
const KEY_TZ: u8 = 13;

// Per-book bookmark table: 8 slots, 3 keys each.
// Slot i: hash at KEY_BM_HASH+i, chapter at KEY_BM_CHAP+i, anchor at KEY_BM_ANCH+i.
// hash == 0 means the slot is empty.
#[cfg(feature = "esp")]
const NUM_BOOKMARKS: usize = 8;
#[cfg(feature = "esp")]
const KEY_BM_HASH: u8 = 20; // 20..27
#[cfg(feature = "esp")]
const KEY_BM_CHAP: u8 = 30; // 30..37
#[cfg(feature = "esp")]
const KEY_BM_ANCH: u8 = 40; // 40..47

#[cfg(feature = "esp")]
struct FlashAdapter(FlashStorage);

#[cfg(feature = "esp")]
impl embedded_storage::nor_flash::ErrorType for FlashAdapter {
    type Error = esp_storage::FlashStorageError;
}

#[cfg(feature = "esp")]
impl embedded_storage_async::nor_flash::ReadNorFlash for FlashAdapter {
    const READ_SIZE: usize = FlashStorage::WORD_SIZE as usize;
    async fn read(&mut self, offset: u32, bytes: &mut [u8]) -> Result<(), Self::Error> {
        embedded_storage::nor_flash::ReadNorFlash::read(&mut self.0, offset, bytes)
    }
    fn capacity(&self) -> usize {
        embedded_storage::nor_flash::ReadNorFlash::capacity(&self.0)
    }
}

#[cfg(feature = "esp")]
impl embedded_storage_async::nor_flash::NorFlash for FlashAdapter {
    const WRITE_SIZE: usize = FlashStorage::WORD_SIZE as usize;
    const ERASE_SIZE: usize = FlashStorage::SECTOR_SIZE as usize;
    async fn erase(&mut self, from: u32, to: u32) -> Result<(), Self::Error> {
        embedded_storage::nor_flash::NorFlash::erase(&mut self.0, from, to)
    }
    async fn write(&mut self, offset: u32, bytes: &[u8]) -> Result<(), Self::Error> {
        embedded_storage::nor_flash::NorFlash::write(&mut self.0, offset, bytes)
    }
}

#[cfg(feature = "esp")]
fn block_on<F: core::future::Future>(mut f: F) -> F::Output {
    use core::{
        pin::Pin,
        task::{Context, Poll, RawWaker, RawWakerVTable, Waker},
    };
    static VTABLE: RawWakerVTable =
        RawWakerVTable::new(|p| RawWaker::new(p, &VTABLE), |_| {}, |_| {}, |_| {});
    let waker = unsafe { Waker::from_raw(RawWaker::new(core::ptr::null(), &VTABLE)) };
    let mut cx = Context::from_waker(&waker);
    loop {
        match unsafe { Pin::new_unchecked(&mut f) }.poll(&mut cx) {
            Poll::Ready(v) => return v,
            Poll::Pending => {}
        }
    }
}

/// Returns the UTC offset in minutes from flash. Default: 0 (UTC).
#[cfg(feature = "esp")]
pub fn load_tz_offset() -> i32 {
    flash_load_u32(KEY_TZ).map(|v| v as i32).unwrap_or(0)
}

/// Returns (font_idx, bl_idx, ori_idx, utc_offset_minutes). Defaults: Medium (1), High (2), Portrait (0), UTC (0).
#[cfg(feature = "esp")]
pub fn load_settings() -> (usize, usize, usize, i32) {
    let mut flash = FlashAdapter(FlashStorage::new());
    let mut cache = NoCache::new();
    let mut buf = [0u8; 64];
    let mut load = |key: u8, default: u32| -> u32 {
        match block_on(map::fetch_item::<u8, u32, _>(
            &mut flash, NVS_RANGE, &mut cache, &mut buf, &key,
        )) {
            Ok(Some(v)) => v,
            _ => default,
        }
    };
    let font = load(KEY_FONT, 1) as usize;
    let bl = load(KEY_BL, 2) as usize;
    let ori = load(KEY_ORI, 0) as usize;
    let tz = load(KEY_TZ, 0) as i32;
    log::info!("settings loaded: font={} bl={} ori={} tz={}min", font, bl, ori, tz);
    (font, bl, ori, tz)
}

// ── Bookmark helpers (ESP only) ───────────────────────────────────────────────

/// Read a single u32 from flash. Returns None on missing key or any error.
#[cfg(feature = "esp")]
fn flash_load_u32(key: u8) -> Option<u32> {
    let mut flash = FlashAdapter(FlashStorage::new());
    let mut cache = NoCache::new();
    let mut buf = [0u8; 64];
    block_on(map::fetch_item::<u8, u32, _>(
        &mut flash, NVS_RANGE, &mut cache, &mut buf, &key,
    ))
    .ok()
    .flatten()
}

/// Write a single u32 to flash. Logs a warning on error, never panics.
#[cfg(feature = "esp")]
fn flash_save_u32(key: u8, val: u32) {
    let mut flash = FlashAdapter(FlashStorage::new());
    let mut cache = NoCache::new();
    let mut buf = [0u8; 64];
    if let Err(e) = block_on(map::store_item::<u8, u32, _>(
        &mut flash, NVS_RANGE, &mut cache, &mut buf, &key, &val,
    )) {
        log::warn!("flash save key {} failed: {:?}", key, e);
    }
}

/// FNV-1a 32-bit hash. Zero is reserved for "empty slot" — mapped to 1.
#[cfg(feature = "esp")]
fn bookmark_hash(filename: &str) -> u32 {
    let mut h: u32 = 2166136261;
    for b in filename.bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(16777619);
    }
    if h == 0 {
        1
    } else {
        h
    }
}

/// Look up the bookmark table for `filename`. Returns None if not found or
/// on any flash error.
#[cfg(feature = "esp")]
fn load_bookmark_impl(filename: &str) -> Option<(usize, usize)> {
    let target = bookmark_hash(filename);
    for i in 0..NUM_BOOKMARKS {
        let hash = flash_load_u32(KEY_BM_HASH + i as u8).unwrap_or(0);
        if hash == target {
            let chapter = flash_load_u32(KEY_BM_CHAP + i as u8).unwrap_or(0) as usize;
            let anchor = flash_load_u32(KEY_BM_ANCH + i as u8).unwrap_or(0) as usize;
            log::info!(
                "bookmark loaded: {:?} ch={} anchor={}",
                filename,
                chapter,
                anchor
            );
            return Some((chapter, anchor));
        }
    }
    None
}

/// Save a bookmark for `filename`. Finds an existing slot (updates in place),
/// a free slot (fills it), or evicts slot 0 if the table is full.
#[cfg(feature = "esp")]
fn save_bookmark_impl(filename: &str, chapter_idx: usize, anchor_byte: usize) {
    let target = bookmark_hash(filename);
    // Scan for matching or empty slot.
    let mut write_slot: Option<usize> = None;
    for i in 0..NUM_BOOKMARKS {
        let hash = flash_load_u32(KEY_BM_HASH + i as u8).unwrap_or(0);
        if hash == target {
            write_slot = Some(i);
            break;
        }
        if hash == 0 && write_slot.is_none() {
            write_slot = Some(i);
        }
    }
    let slot = write_slot.unwrap_or(0); // evict slot 0 when table is full
    flash_save_u32(KEY_BM_HASH + slot as u8, target);
    flash_save_u32(KEY_BM_CHAP + slot as u8, chapter_idx as u32);
    flash_save_u32(KEY_BM_ANCH + slot as u8, anchor_byte as u32);
    log::info!(
        "bookmark saved: {:?} slot={} ch={} anchor={}",
        filename,
        slot,
        chapter_idx,
        anchor_byte
    );
}

/// Persist font/backlight/orientation/timezone settings to flash.
#[cfg(feature = "esp")]
fn save_settings_impl(font_idx: usize, bl_idx: usize, ori_idx: usize, tz_offset_minutes: i32) {
    let mut flash = FlashAdapter(FlashStorage::new());
    let mut cache = NoCache::new();
    let mut buf = [0u8; 64];
    let mut save = |key: u8, val: u32| {
        if let Err(e) = block_on(map::store_item::<u8, u32, _>(
            &mut flash, NVS_RANGE, &mut cache, &mut buf, &key, &val,
        )) {
            log::warn!("flash save key {} failed: {:?}", key, e);
        }
    };
    save(KEY_FONT, font_idx as u32);
    save(KEY_BL, bl_idx as u32);
    save(KEY_ORI, ori_idx as u32);
    save(KEY_TZ, tz_offset_minutes as u32);
}

/// Position to restore for the built-in embedded epub on cold boot.
/// Returns (0, 0) when no bookmark exists yet.
#[cfg(feature = "esp")]
pub fn load_cold_boot_position() -> (usize, usize) {
    load_bookmark_impl("__embedded__").unwrap_or((0, 0))
}

/// Persist the last-read SD card filename to NVS so it can be restored on
/// sleep wakeup. Sentinels like "__welcome__" store length=0 (no restore).
/// NVS key 50 = byte length; keys 51-66 = filename bytes packed as u32s (LE).
/// Silently truncates filenames longer than 64 bytes.
#[cfg(feature = "esp")]
fn save_last_filename_impl(filename: &str) {
    let bytes = filename.as_bytes();
    let len = if filename.starts_with("__") {
        0
    } else {
        bytes.len().min(64)
    };
    flash_save_u32(50, len as u32);
    for i in 0..((len + 3) / 4) {
        let mut chunk = [0u8; 4];
        let start = i * 4;
        let end = (start + 4).min(len);
        chunk[..end - start].copy_from_slice(&bytes[start..end]);
        flash_save_u32(51 + i as u8, u32::from_le_bytes(chunk));
    }
}

/// Read back the filename persisted by `HardwareAccess::save_last_filename`.
/// Returns `None` if nothing was saved or the stored bytes are not valid UTF-8.
#[cfg(feature = "esp")]
pub fn load_last_filename() -> Option<String> {
    let len = flash_load_u32(50)? as usize;
    if len == 0 || len > 64 {
        return None;
    }
    let mut bytes = alloc::vec![0u8; len];
    for i in 0..((len + 3) / 4) {
        let chunk = flash_load_u32(51 + i as u8)?.to_le_bytes();
        let start = i * 4;
        let end = (start + 4).min(len);
        bytes[start..end].copy_from_slice(&chunk[..end - start]);
    }
    String::from_utf8(bytes).ok()
}

// ── Flash writes: relayed to core 0 ───────────────────────────────────────────
//
// The ESP32-S3's ROM SPI flash write/erase routines disable the flash cache
// chip-wide for the duration of the operation. That's harmless when only one
// core is running (the case before `ereader_ui` started using both cores),
// but once `ui_task` runs on core 1 while core 0 keeps executing WiFi/battery/
// time tasks fetched from that same flash, a write issued from core 1 stalls
// core 0 mid-instruction-fetch — and since embassy's cross-core critical
// section is just a spinlock (see `esp-sync`), core 0 tasks that then try to
// take *any* critical section (logging, `Signal`, etc.) spin forever waiting
// for a lock core 1 will never release. The whole chip appears to hang.
//
// So every flash write is funneled through this channel to `flash_write_task`,
// which must be spawned on core 0's executor (alongside `book_load_task`).
//
// That alone isn't sufficient, though: the same cache-disable also corrupts
// *reads* on whichever core ISN'T doing the write — including core 1 reading
// flash-resident font glyph data mid-render, which crashed/reset the board
// even after writes moved to core 0. `ui_task` must therefore also await
// `wait_for_flash_write` (draining anything queued so far) immediately
// before every display flush, so core 1 is suspended — not fetching
// anything — for the whole duration of any write core 0 performs.
#[cfg(feature = "esp")]
use core::sync::atomic::{AtomicU32, Ordering};
#[cfg(feature = "esp")]
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel, signal::Signal};

#[cfg(feature = "esp")]
pub enum FlashWriteRequest {
    Bookmark { filename: String, chapter_idx: usize, anchor_byte: usize },
    LastFilename(String),
    Settings { font_idx: usize, bl_idx: usize, ori_idx: usize, tz_offset_minutes: i32 },
    /// Last-filename + bookmark, saved together right before deep sleep.
    PrepareSleep { filename: String, chapter_idx: usize, anchor_byte: usize },
}

#[cfg(feature = "esp")]
struct FlashWriteMsg {
    id: u32,
    req: FlashWriteRequest,
}

#[cfg(feature = "esp")]
static FLASH_WRITE_SEQ: AtomicU32 = AtomicU32::new(0);
#[cfg(feature = "esp")]
static FLASH_WRITE_CHANNEL: Channel<CriticalSectionRawMutex, FlashWriteMsg, 8> = Channel::new();
/// Signaled with a request's `id` once `flash_write_task` finishes it.
#[cfg(feature = "esp")]
static FLASH_WRITE_DONE: Signal<CriticalSectionRawMutex, u32> = Signal::new();

/// Queue a flash write for `flash_write_task` to perform on core 0. Never
/// blocks; drops the request (with a warning) if the queue is full, which
/// would mean 8 writes are already backed up — not expected in practice.
#[cfg(feature = "esp")]
fn enqueue_flash_write(req: FlashWriteRequest) -> u32 {
    let id = FLASH_WRITE_SEQ.fetch_add(1, Ordering::Relaxed) + 1;
    if FLASH_WRITE_CHANNEL.try_send(FlashWriteMsg { id, req }).is_err() {
        log::warn!("flash write queue full; dropping request {}", id);
    }
    id
}

/// The id of the most recently *enqueued* flash write (0 if none yet).
/// Compare against a previously-observed id to see whether new writes have
/// been queued since; pass the current value to [`wait_for_flash_write`] to
/// drain everything queued so far.
#[cfg(feature = "esp")]
pub fn latest_flash_write_id() -> u32 {
    FLASH_WRITE_SEQ.load(Ordering::Relaxed)
}

/// Wait until request `id` (and, by FIFO order, everything queued before it)
/// has been written. Only call this with an id you haven't already waited
/// for — `Signal` holds a single pending value, so waiting again for an id
/// that was already observed (with nothing newer queued since) blocks
/// forever, since nothing will signal it a second time.
///
/// Used both right before deep sleep (which has no "next tick" to catch up
/// on afterward) and by `ui_task` right before every display flush, since a
/// flash write racing a core-1 render corrupts the flash cache for whichever
/// core isn't the one doing the write (see the module comment above).
#[cfg(feature = "esp")]
pub async fn wait_for_flash_write(id: u32) {
    loop {
        if FLASH_WRITE_DONE.wait().await >= id {
            return;
        }
    }
}

/// Persist the last-read filename and bookmark together, and wait for the
/// write to actually land on flash. Call this instead of the `HardwareAccess`
/// save methods immediately before `enter_deep_sleep` — those are
/// fire-and-forget, but deep sleep powers down before core 0 would get a
/// chance to catch up.
#[cfg(feature = "esp")]
pub async fn save_before_sleep(filename: &str, chapter_idx: usize, anchor_byte: usize) {
    let id = enqueue_flash_write(FlashWriteRequest::PrepareSleep {
        filename: String::from(filename),
        chapter_idx,
        anchor_byte,
    });
    wait_for_flash_write(id).await;
}

/// Runs on core 0 (spawned alongside `book_load_task`) and performs every
/// flash write the app makes. See the module comment above for why this must
/// not run on core 1.
#[cfg(feature = "esp")]
#[embassy_executor::task]
pub async fn flash_write_task() {
    loop {
        let msg = FLASH_WRITE_CHANNEL.receive().await;
        match msg.req {
            FlashWriteRequest::Bookmark { filename, chapter_idx, anchor_byte } => {
                save_bookmark_impl(&filename, chapter_idx, anchor_byte);
            }
            FlashWriteRequest::LastFilename(filename) => {
                save_last_filename_impl(&filename);
            }
            FlashWriteRequest::Settings { font_idx, bl_idx, ori_idx, tz_offset_minutes } => {
                save_settings_impl(font_idx, bl_idx, ori_idx, tz_offset_minutes);
            }
            FlashWriteRequest::PrepareSleep { filename, chapter_idx, anchor_byte } => {
                save_last_filename_impl(&filename);
                save_bookmark_impl(&filename, chapter_idx, anchor_byte);
            }
        }
        FLASH_WRITE_DONE.signal(msg.id);
    }
}

// ── ESP implementation ────────────────────────────────────────────────────────

#[cfg(feature = "esp")]
use esp_hal::{
    gpio::{Input, WakeEvent},
    ledc::{channel::ChannelIFace, LowSpeed},
    rtc_cntl::{
        sleep::{Ext0WakeupSource, GpioWakeupSource, WakeupLevel},
        Rtc,
    },
};
use log::info;

#[cfg(feature = "esp")]
const BL_DUTY: [u8; 3] = [0, 25, 100];

/// Read a 32-bit value from an RTC store register. Registers survive deep sleep.
/// Valid indices: 0, 5, 6.
#[cfg(feature = "esp")]
pub fn rtc_store_read(idx: u8) -> u32 {
    let r = esp_hal::peripherals::LPWR::regs();
    match idx {
        0 => r.store0().read().data().bits(),
        5 => r.store5().read().data().bits(),
        6 => r.store6().read().data().bits(),
        _ => 0,
    }
}

/// Write a 32-bit value to an RTC store register. Survives deep sleep.
/// Valid indices: 0, 5, 6.
#[cfg(feature = "esp")]
pub fn rtc_store_write(idx: u8, val: u32) {
    let r = esp_hal::peripherals::LPWR::regs();
    match idx {
        0 => {
            r.store0().write(|w| unsafe { w.data().bits(val) });
        }
        5 => {
            r.store5().write(|w| unsafe { w.data().bits(val) });
        }
        6 => {
            r.store6().write(|w| unsafe { w.data().bits(val) });
        }
        _ => {}
    }
}

/// ESP32-S3 hardware implementation. Generic over the LEDC channel type so the
/// caller doesn't need to name the concrete channel type from esp-hal.
#[cfg(feature = "esp")]
pub struct EspHardware<'d, C: ChannelIFace<'d, LowSpeed>> {
    font_size: FontSize,
    backlight: BacklightLevel,
    orientation: Orientation,
    utc_offset_minutes: i32,
    bl_ch: C,
    rtc: Rtc<'d>,
    btn_prev: Input<'d>,
    btn_next: Input<'d>,
    cached_battery: BatteryInfo,
}

#[cfg(feature = "esp")]
impl<'d, C: ChannelIFace<'d, LowSpeed>> EspHardware<'d, C> {
    pub fn new(
        bl_ch: C,
        rtc: Rtc<'d>,
        btn_prev: Input<'d>,
        btn_next: Input<'d>,
        font_size: FontSize,
        backlight: BacklightLevel,
        orientation: Orientation,
        utc_offset_minutes: i32,
    ) -> Self {
        bl_ch.set_duty(BL_DUTY[backlight as usize]).unwrap();
        Self {
            font_size,
            backlight,
            orientation,
            utc_offset_minutes,
            bl_ch,
            rtc,
            btn_prev,
            btn_next,
            cached_battery: BatteryInfo { percent: 0, voltage_mv: 0, is_charging: false },
        }
    }

    /// Update cached battery state from parsed hardware readings.
    pub fn update_battery(&mut self, voltage_mv: u32, percent: u8, is_charging: bool) {
        self.cached_battery = BatteryInfo { percent: percent.min(100), voltage_mv, is_charging };
    }
}

#[cfg(feature = "esp")]
impl<'d, C: ChannelIFace<'d, LowSpeed>> HardwareAccess for EspHardware<'d, C> {
    fn font_size(&self) -> FontSize {
        self.font_size
    }
    fn backlight_level(&self) -> BacklightLevel {
        self.backlight
    }
    fn orientation(&self) -> Orientation {
        self.orientation
    }

    fn current_time_secs(&self) -> u64 {
        self.rtc.current_time_us() / 1_000_000
    }

    fn set_font_size(&mut self, size: FontSize) {
        self.font_size = size;
    }

    fn set_backlight_level(&mut self, level: BacklightLevel) {
        self.bl_ch.set_duty(BL_DUTY[level as usize]).unwrap();
        self.backlight = level;
    }

    fn set_orientation(&mut self, orientation: Orientation) {
        self.orientation = orientation;
    }

    fn button_prev_pressed(&self) -> bool {
        self.btn_prev.is_low()
    }

    fn button_next_pressed(&self) -> bool {
        self.btn_next.is_low()
    }

    fn set_current_time_secs(&mut self, secs: u64) {
        self.rtc.set_current_time_us(secs * 1_000_000);
    }

    fn utc_offset_minutes(&self) -> i32 {
        self.utc_offset_minutes
    }

    fn set_utc_offset_minutes(&mut self, minutes: i32) {
        self.utc_offset_minutes = minutes;
    }

    /// Save state to RTC fast memory, turn off backlight, and enter deep sleep.
    /// Wakes when the BOOT button (GPIO0) is pressed LOW. Never returns on ESP.
    fn enter_deep_sleep(&mut self, chapter_idx: usize, anchor_byte: usize) {
        // Pack settings: font (4 bits) | backlight (4 bits, offset 4) | orientation (4 bits, offset 8)
        rtc_store_write(0, anchor_byte as u32);
        rtc_store_write(
            5,
            self.font_size.to_index() as u32
                | ((self.backlight.to_index() as u32) << 4)
                | ((self.orientation.to_index() as u32) << 8),
        );
        rtc_store_write(6, chapter_idx as u32);
        // Turn off backlight PWM
        self.bl_ch.set_duty(0).unwrap();
        // Wake on BOOT button (GPIO0 active-low); steal a new handle since we're about to sleep
        let wakeup_pin = unsafe { esp_hal::gpio::AnyPin::steal(0) };
        let boot_src = Ext0WakeupSource::new(wakeup_pin, WakeupLevel::Low);
        self.rtc.sleep_deep(&[&boot_src]);
    }

    fn enter_light_sleep(&mut self) {
        self.bl_ch.set_duty(0).unwrap();
        self.btn_prev
            .wakeup_enable(true, WakeEvent::LowLevel)
            .unwrap();
        self.btn_next
            .wakeup_enable(true, WakeEvent::LowLevel)
            .unwrap();
        let gpio_src = GpioWakeupSource::new();
        self.rtc.sleep_light(&[&gpio_src]);
        self.bl_ch
            .set_duty(BL_DUTY[self.backlight as usize])
            .unwrap();
    }

    fn save_bookmark(&mut self, filename: &str, chapter_idx: usize, anchor_byte: usize) {
        enqueue_flash_write(FlashWriteRequest::Bookmark {
            filename: String::from(filename),
            chapter_idx,
            anchor_byte,
        });
    }

    fn load_bookmark(&self, filename: &str) -> Option<(usize, usize)> {
        load_bookmark_impl(filename)
    }

    fn save_last_filename(&mut self, filename: &str) {
        enqueue_flash_write(FlashWriteRequest::LastFilename(String::from(filename)));
    }

    fn save_settings(&mut self) {
        enqueue_flash_write(FlashWriteRequest::Settings {
            font_idx: self.font_size.to_index(),
            bl_idx: self.backlight.to_index(),
            ori_idx: self.orientation.to_index(),
            tz_offset_minutes: self.utc_offset_minutes,
        });
    }

    fn list_book_files(&self) -> Vec<String> {
        use embedded_hal::spi::SpiBus;
        use embedded_hal_bus::spi::ExclusiveDevice;
        use embedded_sdmmc::{Error, LfnBuffer, SdCard, SdCardError, VolumeIdx, VolumeManager};
        use esp_hal::{
            delay::Delay,
            gpio::{Level, Output, OutputConfig},
            spi::master::{Config as SpiConfig, Spi},
            time::Rate,
        };

        // SD and LoRa share the SPI bus; deselect both CS lines before touching the bus.
        let _lora_cs = unsafe {
            Output::new(
                esp_hal::gpio::AnyPin::steal(46),
                Level::High,
                OutputConfig::default(),
            )
        };
        let cs = unsafe {
            Output::new(
                esp_hal::gpio::AnyPin::steal(12),
                Level::High,
                OutputConfig::default(),
            )
        };
        let mut spi = unsafe {
            Spi::new(
                esp_hal::peripherals::SPI2::steal(),
                SpiConfig::default().with_frequency(Rate::from_khz(400)),
            )
            .expect("SPI2 init")
            .with_sck(esp_hal::gpio::AnyPin::steal(14))
            .with_mosi(esp_hal::gpio::AnyPin::steal(13))
            .with_miso(esp_hal::gpio::AnyPin::steal(21))
        };
        // SD cards need ≥74 clock cycles with CS HIGH before CMD0 to enter SPI mode.
        // ExclusiveDevice asserts CS for every transaction so we must do this on the raw bus.
        let _ = SpiBus::write(&mut spi, &[0xFF; 10]);
        let Ok(spi_dev) = ExclusiveDevice::new(spi, cs, Delay::new()) else {
            return alloc::vec::Vec::new();
        };
        let sdcard = SdCard::new(spi_dev, Delay::new());
        let mgr = VolumeManager::<_, _, 16, 4, 1>::new_with_limits(sdcard, DummyTimesource, 0);
        let vol = match mgr.open_volume(VolumeIdx(0)) {
            Ok(v) => v,
            Err(Error::DeviceError(SdCardError::CardNotFound)) => {
                log::info!("SD: no card");
                return alloc::vec::Vec::new();
            }
            Err(e) => {
                log::warn!("SD error: {:?}", e);
                return alloc::vec::Vec::new();
            }
        };
        let Ok(root) = vol.open_root_dir() else {
            return alloc::vec::Vec::new();
        };

        let mut files = alloc::vec::Vec::new();
        let mut lfn_storage = [0u8; 256];
        let mut lfn_buf = LfnBuffer::new(&mut lfn_storage);
        let _ = root.iterate_dir_lfn(&mut lfn_buf, |e, lfn| {
            if e.attributes.is_volume() || e.attributes.is_directory() {
                return;
            }
            // Skip Apple Double files (macOS metadata):
            // - with LFN they begin with "._"
            // - without LFN their SFN begins with '_' (8.3 mangling of "._")
            let is_apple_double = match lfn {
                Some(s) => s.starts_with("._"),
                None => alloc::format!("{}", e.name).starts_with('_'),
            };
            if is_apple_double {
                return;
            }
            let ext = e.name.extension();
            if ext.eq_ignore_ascii_case(b"EPU")
                || ext.eq_ignore_ascii_case(b"HTM")
                || ext.eq_ignore_ascii_case(b"TXT")
            {
                let name = match lfn {
                    Some(s) => String::from(s),
                    None => alloc::format!("{}", e.name),
                };
                files.push(name);
            }
        });
        files
    }

    fn load_book_file(&self, name: &str) -> Option<alloc::vec::Vec<u8>> {
        sd_read_file(name)
    }

    fn battery_info(&self) -> BatteryInfo {
        self.cached_battery.clone()
    }

    fn memory_info(&self) -> MemoryInfo {
        use esp_alloc::MemoryCapability;
        let stats = esp_alloc::HEAP.stats();
        let mut psram_free = 0usize;
        let mut psram_total = 0usize;
        let mut sram_free = 0usize;
        let mut sram_total = 0usize;
        for region in stats.region_stats.iter().flatten() {
            if region.capabilities.contains(MemoryCapability::External) {
                psram_free = region.free;
                psram_total = region.size;
            } else if region.capabilities.contains(MemoryCapability::Internal) {
                sram_free += region.free;
                sram_total += region.size;
            }
        }
        MemoryInfo {
            psram_free_bytes: psram_free,
            psram_total_bytes: psram_total,
            sram_free_bytes: sram_free,
            sram_total_bytes: sram_total,
        }
    }
}

// ── SD card helpers (ESP only) ────────────────────────────────────────────────

#[cfg(feature = "esp")]
struct DummyTimesource;

#[cfg(feature = "esp")]
impl embedded_sdmmc::TimeSource for DummyTimesource {
    fn get_timestamp(&self) -> embedded_sdmmc::Timestamp {
        embedded_sdmmc::Timestamp {
            year_since_1970: 0,
            zero_indexed_month: 0,
            zero_indexed_day: 0,
            hours: 0,
            minutes: 0,
            seconds: 0,
        }
    }
}

/// Read a file by display name from the SD card.
///
/// Steals SPI2 and the relevant GPIO pins with `unsafe`; callers are responsible
/// for ensuring no other task accesses SPI2 concurrently (safe in Embassy's
/// cooperative single-core executor as long as no other task also steals SPI2).
#[cfg(feature = "esp")]
pub fn sd_read_file(name: &str) -> Option<alloc::vec::Vec<u8>> {
    use embedded_hal::spi::SpiBus;
    use embedded_hal_bus::spi::ExclusiveDevice;
    use embedded_sdmmc::{Error, LfnBuffer, Mode, SdCard, SdCardError, VolumeIdx, VolumeManager};
    use esp_hal::{
        delay::Delay,
        gpio::{Level, Output, OutputConfig},
        spi::master::{Config as SpiConfig, Spi},
        time::Rate,
    };

    let _lora_cs = unsafe {
        Output::new(
            esp_hal::gpio::AnyPin::steal(46),
            Level::High,
            OutputConfig::default(),
        )
    };
    let cs = unsafe {
        Output::new(
            esp_hal::gpio::AnyPin::steal(12),
            Level::High,
            OutputConfig::default(),
        )
    };
    let mut spi = unsafe {
        Spi::new(
            esp_hal::peripherals::SPI2::steal(),
            SpiConfig::default().with_frequency(Rate::from_khz(400)),
        )
        .expect("SPI2 init")
        .with_sck(esp_hal::gpio::AnyPin::steal(14))
        .with_mosi(esp_hal::gpio::AnyPin::steal(13))
        .with_miso(esp_hal::gpio::AnyPin::steal(21))
    };
    let _ = SpiBus::write(&mut spi, &[0xFF; 10]);
    let spi_dev = ExclusiveDevice::new(spi, cs, Delay::new()).ok()?;
    let sdcard = SdCard::new(spi_dev, Delay::new());
    let mgr = VolumeManager::<_, _, 16, 4, 1>::new_with_limits(sdcard, DummyTimesource, 0);
    let vol = match mgr.open_volume(VolumeIdx(0)) {
        Ok(v) => v,
        Err(Error::DeviceError(SdCardError::CardNotFound)) => {
            log::info!("SD: no card");
            return None;
        }
        Err(e) => {
            log::warn!("SD error: {:?}", e);
            return None;
        }
    };
    let root = vol.open_root_dir().ok()?;

    let mut sfn: Option<embedded_sdmmc::ShortFileName> = None;
    let mut lfn_storage = [0u8; 256];
    let mut lfn_buf = LfnBuffer::new(&mut lfn_storage);
    let _ = root.iterate_dir_lfn(&mut lfn_buf, |e, lfn| {
        if sfn.is_some() {
            return;
        }
        let display = match lfn {
            Some(s) => String::from(s),
            None => alloc::format!("{}", e.name),
        };
        if display == name {
            sfn = Some(e.name.clone());
        }
    });
    let mut f = root.open_file_in_dir(sfn?, Mode::ReadOnly).ok()?;
    let file_len = f.length() as usize;
    {
        use esp_alloc::MemoryCapability;
        let stats = esp_alloc::HEAP.stats();
        let total_free: usize = stats.region_stats.iter().flatten().map(|r| r.free).sum();
        log::info!("sd_read_file: file size={} bytes, total heap free={} bytes", file_len, total_free);
    }
    let mut buf = alloc::vec![0u8; file_len];
    f.read(&mut buf).ok()?;
    Some(buf)
}
