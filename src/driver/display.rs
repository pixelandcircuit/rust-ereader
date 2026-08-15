use alloc::boxed::Box;

use embedded_hal::i2c::I2c as I2cTrait;
use esp_hal::delay::Delay;
use log::*;

use crate::driver::{ed047tc1, Error, Result};

// Full-quality 15-frame waveforms (GL16 style).
const CONTRAST_CYCLES_4BPP: &[u16] =
    &[30, 30, 20, 20, 30, 30, 30, 40, 40, 50, 50, 50, 100, 200, 300];
const CONTRAST_CYCLES_4BPP_WHITE: &[u16] =
    &[10, 10, 8, 8, 8, 8, 8, 10, 10, 15, 15, 20, 20, 100, 300];

// Fast 4-frame waveforms for page turns.  Concentrate drive energy into fewer
// cycles at the cost of some ghosting.  Tune on-device if contrast is lacking.
const CONTRAST_CYCLES_FAST: &[u16] = &[30, 50, 150, 300];
const CONTRAST_CYCLES_FAST_WHITE: &[u16] = &[15, 20, 80, 200];

/// Display rotation, only 90° increments supported
#[derive(Clone, Copy, Default)]
pub enum DisplayRotation {
    #[default]
    Rotate0,
    Rotate90,
    Rotate180,
    Rotate270,
}

#[derive(Clone, Copy, Debug)]
pub enum DrawMode {
    BlackOnWhite,
    WhiteOnWhite,
    WhiteOnBlack,
    /// 4-frame fast draw pass — pairs with `FastClear` for quick page turns.
    Fast,
    /// 4-frame fast ghost-clear pass — use before `Fast` instead of `WhiteOnBlack`.
    FastClear,
}

#[derive(Clone, Copy, Debug)]
pub struct Rectangle {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

impl DrawMode {
    fn lut_default(&self) -> u8 {
        match self {
            Self::BlackOnWhite | Self::WhiteOnWhite | Self::Fast => 0x55,
            Self::WhiteOnBlack | Self::FastClear => 0xAA,
        }
    }

    fn contrast_cycles(&self) -> &[u16] {
        match self {
            Self::WhiteOnBlack => CONTRAST_CYCLES_4BPP_WHITE,
            Self::BlackOnWhite | Self::WhiteOnWhite => CONTRAST_CYCLES_4BPP,
            Self::Fast => CONTRAST_CYCLES_FAST,
            Self::FastClear => CONTRAST_CYCLES_FAST_WHITE,
        }
    }

    fn reverse_frames(&self) -> bool {
        matches!(self, Self::BlackOnWhite | Self::WhiteOnWhite | Self::Fast)
    }
}

pub const DISPLAY_WIDTH: u16 = 960;
pub const DISPLAY_HEIGHT: u16 = 540;
const TAINTED_ROWS_SIZE: usize = DISPLAY_HEIGHT as usize / 8 + 1;
const FRAMEBUFFER_SIZE: usize = (DISPLAY_WIDTH / 2) as usize * DISPLAY_HEIGHT as usize;
const BYTES_PER_LINE: usize = DISPLAY_WIDTH as usize / 4;
const LINE_BYTES_4BPP: usize = DISPLAY_WIDTH as usize / 2;

pub struct Display<'a, I> {
    epd: ed047tc1::ED047TC1<'a, I>,
    skipping: u16,
    framebuffer: Box<[u8; FRAMEBUFFER_SIZE]>,
    tainted_rows: [u8; TAINTED_ROWS_SIZE],
    rotation: DisplayRotation,
    /// Pre-allocated 64 KB waveform LUT; reset at the start of each flush to
    /// avoid a heap allocation on every page turn.
    lut: Box<[u8; 1 << 16]>,
}

impl<'a, I: I2cTrait> Display<'a, I> {
    pub const WIDTH: u16 = DISPLAY_WIDTH;
    pub const HEIGHT: u16 = DISPLAY_HEIGHT;
    pub const BOUNDING_BOX: Rectangle = Rectangle {
        x: 0,
        y: 0,
        width: DISPLAY_WIDTH,
        height: DISPLAY_HEIGHT,
    };

    pub fn new(
        pins: ed047tc1::PinConfig<'a>,
        dma: esp_hal::peripherals::DMA_CH0<'a>,
        lcd_cam: esp_hal::peripherals::LCD_CAM<'a>,
        rmt: esp_hal::peripherals::RMT<'a>,
        i2c: I,
    ) -> Result<Self> {
        Ok(Display {
            epd: ed047tc1::ED047TC1::new(pins, dma, lcd_cam, rmt, i2c)?,
            skipping: 0,
            framebuffer: Box::new([0xFF; FRAMEBUFFER_SIZE]),
            tainted_rows: [0; TAINTED_ROWS_SIZE],
            rotation: DisplayRotation::default(),
            lut: Box::new([0u8; 1 << 16]),
        })
    }

    pub fn set_rotation(&mut self, rotation: DisplayRotation) {
        self.rotation = rotation;
    }

    pub fn rotation(&self) -> DisplayRotation {
        self.rotation
    }

    pub fn power_on(&mut self) {
        debug!("Display power on");
        self.epd.power_on()
    }

    pub fn power_off(&mut self) {
        debug!("Display power off");
        self.epd.power_off()
    }

    pub fn set_pixel(&mut self, x: u16, y: u16, color: u8) -> Result<()> {
        if x >= DISPLAY_WIDTH || y >= DISPLAY_HEIGHT {
            return Err(Error::OutOfBounds);
        }
        if color > 0x0F {
            return Err(Error::InvalidColor);
        }
        let index: usize = x as usize / 2 + y as usize * (DISPLAY_WIDTH as usize / 2);
        let value = self.framebuffer[index];
        if x % 2 == 1 {
            self.framebuffer[index] = (value & 0x0F) | ((color << 4) & 0xF0);
        } else {
            self.framebuffer[index] = (value & 0xF0) | (color & 0x0F);
        }
        let tainted_index = y as usize / 8;
        self.tainted_rows[tainted_index] |= 1 << (y % 8);
        Ok(())
    }

    pub fn fill(&mut self, color: u8) -> Result<()> {
        debug!("display fill");
        if color > 0x0F {
            return Err(Error::InvalidColor);
        }
        self.framebuffer.fill(color << 4 | color);
        self.tainted_rows.fill(0xFF);
        Ok(())
    }

    pub fn fill_region(&mut self, area: Rectangle, color: u8) -> Result<()> {
        if color > 0x0F {
            return Err(Error::InvalidColor);
        }
        let x_end = (area.x + area.width).min(DISPLAY_WIDTH);
        let y_end = (area.y + area.height).min(DISPLAY_HEIGHT);
        for y in area.y..y_end {
            for x in area.x..x_end {
                self.set_pixel(x, y, color).ok();
            }
        }
        Ok(())
    }

    pub fn flush(&mut self, mode: DrawMode) -> Result<()> {
        debug!("display flush");
        self.draw(mode)?;
        self.tainted_rows.fill(0);
        self.framebuffer.fill(0xFF);
        Ok(())
    }

    pub fn clear(&mut self) -> Result<()> {
        debug!("display clear");
        self.clear_area(Self::BOUNDING_BOX)
    }

    /// Multiple alternating black/white full-refresh passes to discharge residual
    /// charge imbalance that builds up as faint lines during repeated partial refreshes.
    pub fn deep_clean(&mut self, cycles: u8) -> Result<()> {
        debug!("display deep_clean cycles={}", cycles);
        for _ in 0..cycles {
            self.fill(0x00)?;
            self.flush(DrawMode::BlackOnWhite)?;
            self.fill(0x0F)?;
            self.flush(DrawMode::WhiteOnBlack)?;
        }
        Ok(())
    }

    /// Read touch and face-button key state in one I2C transaction.
    /// Returns `(touch_point, face_button_pressed)`.
    pub fn read_touch_and_key(
        &mut self,
        gt911: &mut crate::driver::gt911::Gt911,
    ) -> (Option<(u16, u16)>, bool) {
        gt911.read_input(self.epd.i2c())
    }

    /// Poll just the GT911 face-button key state (for use inside a hold loop).
    pub fn gt911_key_pressed(&mut self, gt911: &mut crate::driver::gt911::Gt911) -> bool {
        gt911.key_pressed(self.epd.i2c())
    }

    /// Poll the GT911 touch controller for the first active touch point.
    /// Returns `Some((x, y))` when a finger is down, `None` otherwise.
    pub fn read_touch(&mut self, gt911: &mut crate::driver::gt911::Gt911) -> Option<(u16, u16)> {
        gt911.read_touch(self.epd.i2c())
    }

    /// Write a valid GT911 configuration block so it starts scanning.
    /// Call this if the GT911 was never configured (config version = 0x00).
    pub fn configure_touch(
        &mut self,
        gt911: &mut crate::driver::gt911::Gt911,
        x_max: u16,
        y_max: u16,
    ) {
        gt911.configure(self.epd.i2c(), x_max, y_max);
    }

    /// Clear the GT911 buffer-ready flag and set coordinate-output mode.
    pub fn init_touch(&mut self, gt911: &mut crate::driver::gt911::Gt911) {
        gt911.init(self.epd.i2c());
    }

    /// Read the GT911 4-byte product ID ("911\0" if genuine).
    pub fn touch_product_id(&mut self, gt911: &mut crate::driver::gt911::Gt911) -> [u8; 4] {
        gt911.product_id(self.epd.i2c())
    }

    /// Probe both GT911 I2C addresses and return the one that ACKs.
    /// Returns `None` if no touch controller is found on the bus.
    pub fn detect_touch_addr(&mut self) -> Option<u8> {
        crate::driver::gt911::Gt911::detect(self.epd.i2c())
    }

    /// Read GT911 config registers for diagnostics.
    /// Returns [version, x_lo, x_hi, y_lo, y_hi, max_touch, int_mode]
    pub fn touch_read_config(&mut self, gt911: &mut crate::driver::gt911::Gt911) -> [u8; 7] {
        gt911.read_config(self.epd.i2c())
    }

    /// Read the raw GT911 status register byte for diagnostics.
    pub fn touch_read_status_raw(&mut self, gt911: &mut crate::driver::gt911::Gt911) -> u8 {
        gt911.read_status_raw(self.epd.i2c())
    }

    /// Write 0x00 to the GT911 status register to clear the buffer-ready flag.
    pub fn touch_clear_status(&mut self, gt911: &mut crate::driver::gt911::Gt911) {
        gt911.clear_status(self.epd.i2c())
    }

    /// Read one byte from an arbitrary I2C device on the shared bus.
    pub fn i2c_read_u8(&mut self, addr: u8, reg: u8) -> u8 {
        let i2c = self.epd.i2c();
        let mut buf = [0u8; 1];
        let _ = i2c.write_read(addr, &[reg], &mut buf);
        buf[0]
    }

    /// Read two bytes (little-endian u16) from an arbitrary I2C device on the shared bus.
    pub fn i2c_read_u16(&mut self, addr: u8, reg: u8) -> u16 {
        let i2c = self.epd.i2c();
        let mut buf = [0u8; 2];
        let _ = i2c.write_read(addr, &[reg], &mut buf);
        u16::from_le_bytes(buf)
    }

    /// Read two bytes (little-endian i16) from an arbitrary I2C device on the shared bus.
    pub fn i2c_read_i16(&mut self, addr: u8, reg: u8) -> i16 {
        let i2c = self.epd.i2c();
        let mut buf = [0u8; 2];
        let _ = i2c.write_read(addr, &[reg], &mut buf);
        i16::from_le_bytes(buf)
    }

    /// Scan all 7-bit I2C addresses and print those that ACK (for diagnostics).
    pub fn i2c_scan(&mut self) {
        let i2c = self.epd.i2c();
        for addr in 0x00u8..=0x7F {
            let mut buf = [0u8; 1];
            if i2c.read(addr, &mut buf).is_ok() {
                esp_println::println!("  I2C ACK at 0x{:02X}", addr);
            }
        }
    }

    pub fn repair(&mut self, delay: Delay) -> Result<()> {
        debug!("display repair");
        self.clear()?;
        for _ in 0..20 {
            self.push_pixels(Self::BOUNDING_BOX, 50, 0)?;
            delay.delay_millis(500);
        }
        self.clear()?;
        for _ in 0..40 {
            self.push_pixels(Self::BOUNDING_BOX, 50, 1)?;
            delay.delay_millis(500);
        }
        self.clear()
    }

    pub fn clear_area(&mut self, area: Rectangle) -> Result<()> {
        self.clear_cycles(area, 4, 50)
    }

    fn clear_cycles(&mut self, area: Rectangle, cycles: u16, cycle_time: u16) -> Result<()> {
        for _ in 0..cycles {
            for _ in 0..4 {
                self.push_pixels(area, cycle_time, 0)?;
            }
            for _ in 0..4 {
                self.push_pixels(area, cycle_time, 1)?;
            }
        }
        Ok(())
    }

    fn push_pixels(&mut self, area: Rectangle, time: u16, color: u16) -> Result<()> {
        let mut row = [0u8; BYTES_PER_LINE];

        for i in 0..area.width {
            let pos = i + area.x % 4;
            let mask = match color {
                1 => 0b10101010,
                _ => 0b01010101,
            } & (0b00000011 << (2 * (pos % 4)));
            row[(area.x / 4 + pos / 4) as usize] |= mask;
        }
        line_buffer_reorder(&mut row);
        self.epd.frame_start()?;

        for i in 0..DISPLAY_HEIGHT {
            if i < area.y {
                self.row_skip(time)?;
                continue;
            }
            if i == area.y {
                self.epd.set_buffer(&row)?;
                self.row_write(time)?;
                continue;
            }
            if i >= area.y + area.height {
                self.row_skip(time)?;
                continue;
            }
            self.row_write(time)?;
        }
        self.row_write(time)?;
        self.epd.frame_end()?;

        Ok(())
    }

    fn row_skip(&mut self, output_time: u16) -> Result<()> {
        match self.skipping {
            0 => {
                self.epd.set_buffer(&[0u8; BYTES_PER_LINE])?;
                self.epd.output_row(output_time)?;
            }
            i if i < 2 => {
                self.epd.output_row(10)?;
            }
            _ => {
                self.epd.skip()?;
            }
        }
        self.skipping += 1;
        Ok(())
    }

    fn row_write(&mut self, output_time: u16) -> Result<()> {
        self.skipping = 0;
        self.epd.output_row(output_time)?;
        Ok(())
    }

    fn is_tainted(&self, row: u16) -> bool {
        let index = row as usize / 8;
        self.tainted_rows[index] & (1 << (row % 8)) != 0
    }

    fn draw(&mut self, mode: DrawMode) -> Result<()> {
        // Find the contiguous physical row range that needs updating.
        // Rows outside this range get fast CKV skips to avoid driving untouched pixels.
        let mut row_start = DISPLAY_HEIGHT;
        let mut row_end = 0u16;
        for y in 0..DISPLAY_HEIGHT {
            if self.is_tainted(y) {
                if y < row_start {
                    row_start = y;
                }
                if y + 1 > row_end {
                    row_end = y + 1;
                }
            }
        }
        if row_start >= DISPLAY_HEIGHT {
            return Ok(()); // nothing tainted
        }

        let frame_count = mode.contrast_cycles().len();
        self.lut.fill(mode.lut_default());

        for k in 0..frame_count {
            update_lut(&mut self.lut[..], k, mode);
            self.skipping = 0;
            self.epd.frame_start()?;
            for y in 0..DISPLAY_HEIGHT {
                if y < row_start || y >= row_end {
                    self.epd.skip()?;
                    continue;
                }
                if !self.is_tainted(y) {
                    self.row_skip(mode.contrast_cycles()[k])?;
                    continue;
                }
                let row_offset = y as usize * LINE_BYTES_4BPP;
                let mut dma_buf = [0u8; BYTES_PER_LINE];
                prepare_dma_buffer(
                    &self.framebuffer[row_offset..row_offset + LINE_BYTES_4BPP],
                    &self.lut[..],
                    &mut dma_buf,
                );
                self.epd.set_buffer(&dma_buf)?;
                self.row_write(mode.contrast_cycles()[k])?;
            }
            if self.skipping == 0 {
                self.row_write(mode.contrast_cycles()[k])?;
            }
            self.epd.frame_end()?;
        }
        Ok(())
    }

    /// Like `flush`, but applies the waveform only within `area` (physical coords).
    /// Rows outside area.y..area.y+area.height get fast CKV skips.
    /// Columns outside area.x..area.x+area.width are zeroed in the DMA buffer (VCOM = no drive),
    /// so display content outside the column range is left physically unchanged.
    pub fn flush_region(&mut self, area: Rectangle, mode: DrawMode) -> Result<()> {
        debug!("display flush_region");
        let row_start = area.y.min(DISPLAY_HEIGHT);
        let row_end = (area.y + area.height).min(DISPLAY_HEIGHT);
        let col_start = area.x as usize;
        let col_end = (area.x + area.width).min(DISPLAY_WIDTH) as usize;

        let frame_count = mode.contrast_cycles().len();
        self.lut.fill(mode.lut_default());
        for k in 0..frame_count {
            update_lut(&mut self.lut[..], k, mode);
            self.skipping = 0;
            self.epd.frame_start()?;
            for y in 0..DISPLAY_HEIGHT {
                if y < row_start || y >= row_end {
                    // row_skip() writes VCOM for the first two skipped rows then falls
                    // back to CKV-only once the output latch is neutralized.  The raw
                    // epd.skip() here would leave the latch holding whatever waveform
                    // the last in-range row drove, causing overdraw outside the dialog.
                    self.row_skip(mode.contrast_cycles()[k])?;
                    continue;
                }
                let row_offset = y as usize * LINE_BYTES_4BPP;
                let mut dma_buf = [0u8; BYTES_PER_LINE];
                prepare_dma_buffer(
                    &self.framebuffer[row_offset..row_offset + LINE_BYTES_4BPP],
                    &self.lut[..],
                    &mut dma_buf,
                );
                mask_dma_columns(&mut dma_buf, col_start, col_end);
                self.epd.set_buffer(&dma_buf)?;
                self.row_write(mode.contrast_cycles()[k])?;
            }
            if self.skipping == 0 {
                self.row_write(mode.contrast_cycles()[k])?;
            }
            self.epd.frame_end()?;
        }
        self.tainted_rows.fill(0);
        self.framebuffer.fill(0xFF);
        Ok(())
    }
}

/// Zero out the 2-bit waveform values for pixel columns outside [col_start, col_end).
/// After `prepare_dma_buffer`, each byte holds 4 pixels in MSB-first 2-bit pairs:
///   bits 7-6 = column 4*byte_idx+0, bits 5-4 = +1, bits 3-2 = +2, bits 1-0 = +3.
/// Setting a pair to 0b00 applies VCOM (no drive) to that column.
fn mask_dma_columns(buf: &mut [u8], col_start: usize, col_end: usize) {
    for c in 0..DISPLAY_WIDTH as usize {
        if c < col_start || c >= col_end {
            let byte_idx = c / 4;
            let bit_shift = 6 - 2 * (c % 4);
            buf[byte_idx] &= !(0b11u8 << bit_shift);
        }
    }
}

fn line_buffer_reorder(data: &mut [u8]) {
    for chunk in data.chunks_exact_mut(4) {
        let val = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        let swapped = (val >> 16) | ((val & 0x0000FFFF) << 16);
        chunk.copy_from_slice(&swapped.to_le_bytes());
    }
}

/// Convert one row of 4bpp framebuffer data into a DMA waveform buffer.
/// Writes into `out` (stack-allocated by caller) — no heap allocation.
fn prepare_dma_buffer(line_data: &[u8], conversion_lut: &[u8], out: &mut [u8; BYTES_PER_LINE]) {
    // Each 8-byte chunk of line_data holds 4 pixel-pairs (4 × u16 LUT keys).
    // Each LUT lookup returns one output byte (4 × 2-bit drive codes).
    for (j, chunk) in line_data.chunks(8).enumerate() {
        let v1 = u16::from_le_bytes([chunk[0], chunk[1]]);
        let v2 = u16::from_le_bytes([chunk[2], chunk[3]]);
        let v3 = u16::from_le_bytes([chunk[4], chunk[5]]);
        let v4 = u16::from_le_bytes([chunk[6], chunk[7]]);
        let word: u32 = (conversion_lut[v1 as usize] as u32)
            | (conversion_lut[v2 as usize] as u32) << 8
            | (conversion_lut[v3 as usize] as u32) << 16
            | (conversion_lut[v4 as usize] as u32) << 24;
        out[j * 4..(j + 1) * 4].copy_from_slice(&word.to_le_bytes());
    }
    // ED047TC1 expects MSB-first within each byte (bits 6-7 = leftmost pixel).
    // The LUT produces LSB-first (bits 0-1 = leftmost), so reverse each 2-bit pair.
    for byte in out.iter_mut() {
        let b = *byte;
        *byte = ((b & 0x03) << 6) | ((b & 0x0C) << 2) | ((b & 0x30) >> 2) | ((b & 0xC0) >> 6);
    }
}

// The waveform LUT maps from a 4-pixel group value to 2-bit drive codes. Reversed
// modes (BlackOnWhite, Fast) always count DOWN from frame 15, even when running fewer
// frames — so that white pixels (value 15) are VCOM'd in the very first frame, not
// frame (frame_count-1). Without this, a 4-frame Fast pass would only cover pixel
// values 1-4 with VCOM, leaving value-15 (white bg) driving dark the whole time.
const FULL_FRAME_COUNT: usize = 15;

fn update_lut(conversion_lut: &mut [u8], k: usize, mode: DrawMode) {
    let k = if mode.reverse_frames() { FULL_FRAME_COUNT - k } else { k };
    for l in (k..1 << 16).step_by(16) {
        conversion_lut[l] &= 0xFC;
    }
    for l in ((k << 4)..(1 << 16)).step_by(1 << 8) {
        for p in 0..16 {
            conversion_lut[l + p] &= 0xF3
        }
    }
    for l in ((k << 8)..(1 << 16)).step_by(1 << 12) {
        for p in 0..(1 << 8) {
            conversion_lut[l + p] &= 0xCF
        }
    }
    for l in (k << 12)..((k + 1) << 12) {
        conversion_lut[l] &= 0x3F;
    }
}
