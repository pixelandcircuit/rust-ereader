/// Hardware abstraction for the e-reader device.
///
/// Provides uniform access to font size, backlight level, orientation,
/// and current time across the simulator and the real ESP32-S3 device.

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
            "Land"   => Self::Landscape,
            "R.Port" => Self::ReversePortrait,
            "R.Land" => Self::ReverseLandscape,
            _        => Self::Portrait,
        }
    }

    pub fn to_index(self) -> usize {
        match self {
            Self::Portrait         => 0,
            Self::Landscape        => 1,
            Self::ReversePortrait  => 2,
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
            Self::Portrait         => (h - 1 - ty, tx          ),
            Self::Landscape        => (tx,          ty          ),
            Self::ReversePortrait  => (ty,           w - 1 - tx ),
            Self::ReverseLandscape => (w - 1 - tx,  h - 1 - ty ),
        }
    }

    /// Convert logical pixel coordinates to physical display coordinates.
    pub fn logical_to_phys(self, lx: u16, ly: u16) -> (u16, u16) {
        let w = PANEL_W;
        let h = PANEL_H;
        match self {
            Self::Portrait         => (ly,          h - 1 - lx ),
            Self::Landscape        => (lx,          ly          ),
            Self::ReversePortrait  => (w - 1 - ly,  lx         ),
            Self::ReverseLandscape => (w - 1 - lx,  h - 1 - ly ),
        }
    }
}

// ── Trait ─────────────────────────────────────────────────────────────────────

pub trait HardwareAccess {
    fn font_size(&self) -> FontSize;
    fn backlight_level(&self) -> BacklightLevel;
    fn orientation(&self) -> Orientation;
    fn current_time_secs(&self) -> u64;

    fn set_font_size(&mut self, size: FontSize);
    fn set_backlight_level(&mut self, level: BacklightLevel);
    fn set_orientation(&mut self, orientation: Orientation);
    /// Set the hardware RTC to the given Unix timestamp. No-op on simulator.
    fn set_current_time_secs(&mut self, secs: u64);

    /// Physical BOOT button (GPIO0) state — always false on simulator.
    fn button_prev_pressed(&self) -> bool;
    /// Physical NEXT button (GPIO38) state — always false on simulator.
    fn button_next_pressed(&self) -> bool;
    /// Save reading position and settings to RTC fast memory, turn off
    /// backlight, and enter deep sleep (wakes on BOOT button / GPIO0 LOW).
    /// On ESP this never returns. On simulator it is a no-op.
    fn enter_deep_sleep(&mut self, chapter_idx: usize, anchor_byte: usize);
}

// ── Simulator implementation ──────────────────────────────────────────────────

#[cfg(feature = "simulator")]
pub struct SimHardware {
    font_size: FontSize,
    backlight: BacklightLevel,
    orientation: Orientation,
}

#[cfg(feature = "simulator")]
impl SimHardware {
    pub fn new() -> Self {
        Self {
            font_size: FontSize::Medium,
            backlight: BacklightLevel::High,
            orientation: Orientation::Portrait,
        }
    }
}

#[cfg(feature = "simulator")]
impl HardwareAccess for SimHardware {
    fn font_size(&self) -> FontSize { self.font_size }
    fn backlight_level(&self) -> BacklightLevel { self.backlight }
    fn orientation(&self) -> Orientation { self.orientation }

    fn current_time_secs(&self) -> u64 {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs()
    }

    fn set_font_size(&mut self, size: FontSize) { self.font_size = size; }
    fn set_backlight_level(&mut self, level: BacklightLevel) { self.backlight = level; }
    fn set_orientation(&mut self, orientation: Orientation) { self.orientation = orientation; }
    fn set_current_time_secs(&mut self, _secs: u64) {} // simulator reads from system clock

    fn button_prev_pressed(&self) -> bool { false }
    fn button_next_pressed(&self) -> bool { false }
    fn enter_deep_sleep(&mut self, _chapter_idx: usize, _anchor_byte: usize) {}
}

// ── ESP implementation ────────────────────────────────────────────────────────

#[cfg(feature = "esp")]
use esp_hal::{
    gpio::Input,
    ledc::{channel::ChannelIFace, LowSpeed},
    rtc_cntl::{sleep::{Ext0WakeupSource, WakeupLevel}, Rtc},
};

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
        0 => { r.store0().write(|w| unsafe { w.data().bits(val) }); }
        5 => { r.store5().write(|w| unsafe { w.data().bits(val) }); }
        6 => { r.store6().write(|w| unsafe { w.data().bits(val) }); }
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
    bl_ch: C,
    rtc: Rtc<'d>,
    btn_prev: Input<'d>,
    btn_next: Input<'d>,
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
    ) -> Self {
        bl_ch.set_duty(BL_DUTY[backlight as usize]).unwrap();
        Self { font_size, backlight, orientation, bl_ch, rtc, btn_prev, btn_next }
    }
}

#[cfg(feature = "esp")]
impl<'d, C: ChannelIFace<'d, LowSpeed>> HardwareAccess for EspHardware<'d, C> {
    fn font_size(&self) -> FontSize { self.font_size }
    fn backlight_level(&self) -> BacklightLevel { self.backlight }
    fn orientation(&self) -> Orientation { self.orientation }

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

    /// Save state to RTC fast memory, turn off backlight, and enter deep sleep.
    /// Wakes when the BOOT button (GPIO0) is pressed LOW. Never returns on ESP.
    fn enter_deep_sleep(&mut self, chapter_idx: usize, anchor_byte: usize) {
        // Pack settings: font (4 bits) | backlight (4 bits, offset 4) | orientation (4 bits, offset 8)
        rtc_store_write(0, anchor_byte as u32);
        rtc_store_write(5,
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
}
