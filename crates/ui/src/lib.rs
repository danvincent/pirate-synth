use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use font8x8::UnicodeFonts;
use rppal::gpio::{Gpio, InputPin, OutputPin};
use rppal::spi::{Bus, Mode, SlaveSelect, Spi};

// Keep SPI transfers small to avoid EMSGSIZE from Linux spidev on constrained targets.
const SPI_FRAMEBUFFER_CHUNK_SIZE: usize = 4096;
const SPI_CLOCK_HZ: u32 = 16_000_000;

pub const KEY_NAMES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Up,
    Down,
    Select,
    Back,
}

#[derive(Clone, Debug)]
pub struct MenuState {
    pub key_index: usize,
    pub octave: i32,
    pub fine_tune_cents: f32,
    pub stereo_spread: u8,
    pub selected_item: usize,
    pub scroll_offset: usize,
}

impl MenuState {
    pub fn new(octave: i32, fine_tune_cents: f32) -> Self {
        Self {
            key_index: 0,
            octave,
            fine_tune_cents,
            stereo_spread: 0,
            selected_item: 0,
            scroll_offset: 0,
        }
    }

    pub fn key_name(&self) -> &'static str {
        KEY_NAMES[self.key_index]
    }

    pub fn total_items(&self) -> usize {
        4
    }

    pub fn apply_button(&mut self, button: Button) {
        const VISIBLE_ROWS: usize = 11;
        
        match button {
            Button::Up => {
                if self.selected_item == 0 {
                    self.selected_item = self.total_items().saturating_sub(1);
                } else {
                    self.selected_item -= 1;
                }
            }
            Button::Down => {
                self.selected_item = (self.selected_item + 1) % self.total_items();
            }
            Button::Select => self.increment_selected_value(),
            Button::Back => self.decrement_selected_value(),
        }
        
        // Adjust scroll offset to keep selected_item visible
        if self.selected_item < self.scroll_offset {
            self.scroll_offset = self.selected_item;
        } else if self.selected_item >= self.scroll_offset + VISIBLE_ROWS {
            self.scroll_offset = self.selected_item + 1 - VISIBLE_ROWS;
        }
    }

    fn increment_selected_value(&mut self) {
        match self.selected_item {
            0 => self.key_index = (self.key_index + 1) % KEY_NAMES.len(),
            1 => self.octave = (self.octave + 1).min(8),
            2 => self.fine_tune_cents = (self.fine_tune_cents + 1.0).min(100.0),
            3 => self.stereo_spread = (self.stereo_spread + 5).min(100),
            _ => {}
        }
    }

    fn decrement_selected_value(&mut self) {
        match self.selected_item {
            0 => {
                if self.key_index == 0 {
                    self.key_index = KEY_NAMES.len() - 1;
                } else {
                    self.key_index -= 1;
                }
            }
            1 => self.octave = (self.octave - 1).max(0),
            2 => self.fine_tune_cents = (self.fine_tune_cents - 1.0).max(-100.0),
            3 => self.stereo_spread = self.stereo_spread.saturating_sub(5),
            _ => {}
        }
    }

    pub fn lines(&self) -> Vec<String> {
        vec![
            format!("KEY: {}", self.key_name()),
            format!("OCT: {}", self.octave),
            format!("CENTS: {:+}", self.fine_tune_cents as i32),
            format!("SPREAD: {}", self.stereo_spread),
        ]
    }
}

pub struct ButtonReader {
    pins: [InputPin; 4],
    last: [bool; 4],
}

impl ButtonReader {
    pub fn new() -> Result<Self> {
        let bcm = [5u32, 6, 16, 24];
        let gpio = Gpio::new().context("failed to open GPIO controller")?;
        let pins = [
            open_input_pullup_pin(&gpio, bcm[0])?,
            open_input_pullup_pin(&gpio, bcm[1])?,
            open_input_pullup_pin(&gpio, bcm[2])?,
            open_input_pullup_pin(&gpio, bcm[3])?,
        ];
        Ok(Self {
            pins,
            last: [false; 4],
        })
    }

    pub fn poll_pressed(&mut self) -> Result<Option<Button>> {
        let mapping = [Button::Up, Button::Down, Button::Select, Button::Back];
        for (idx, pin) in self.pins.iter_mut().enumerate() {
            let pressed = pin.is_low();
            let rising = pressed && !self.last[idx];
            self.last[idx] = pressed;
            if rising {
                return Ok(Some(mapping[idx]));
            }
        }
        Ok(None)
    }
}

fn open_input_pullup_pin(gpio: &Gpio, bcm_pin: u32) -> Result<InputPin> {
    let pin = gpio
        .get(bcm_pin as u8)
        .with_context(|| format!("failed to open BCM gpio{bcm_pin}"))?
        .into_input_pullup();
    Ok(pin)
}

pub struct St7789Display {
    spi: Spi,
    dc: OutputPin,
    backlight: Option<OutputPin>,
}

impl St7789Display {
    pub fn new(spi_path: &str, dc_pin: u32, backlight_pin: Option<u32>) -> Result<Self> {
        let (bus, slave_select) = parse_spi_device(spi_path)?;
        let spi = Spi::new(bus, slave_select, SPI_CLOCK_HZ, Mode::Mode0)
            .with_context(|| format!("failed to open SPI device {spi_path}"))?;

        let gpio = Gpio::new().context("failed to open GPIO controller")?;
        let mut dc = gpio
            .get(dc_pin as u8)
            .with_context(|| format!("failed to open BCM gpio{dc_pin} (display DC)"))?
            .into_output();
        dc.set_low();

        let backlight = match backlight_pin {
            Some(pin) => {
                let mut p = gpio
                    .get(pin as u8)
                    .with_context(|| format!("failed to open BCM gpio{pin} (backlight)"))?
                    .into_output();
                p.set_high();
                Some(p)
            }
            None => None,
        };

        let mut display = Self { spi, dc, backlight };
        display.init()?;
        Ok(display)
    }

    fn init(&mut self) -> Result<()> {
        self.command(0x01, &[])?;  // SWRESET
        std::thread::sleep(std::time::Duration::from_millis(150));
        self.command(0x11, &[])?;  // SLPOUT
        std::thread::sleep(std::time::Duration::from_millis(10));
        self.command(0x3A, &[0x55])?;  // COLMOD RGB565
        self.command(0x36, &[0x00])?;  // MADCTL default orientation
        self.command(0x21, &[])?;      // INVON
        self.command(0x13, &[])?;      // NORON
        self.command(0x29, &[])?;      // DISPON
        std::thread::sleep(std::time::Duration::from_millis(10));
        if let Some(backlight) = &mut self.backlight {
            backlight.set_high();
        }
        Ok(())
    }

    fn command(&mut self, cmd: u8, data: &[u8]) -> Result<()> {
        self.dc.set_low();
        self.spi
            .write(&[cmd])
            .map(|_| ())
            .with_context(|| format!("failed writing ST7789 command 0x{cmd:02X}"))?;
        if !data.is_empty() {
            self.dc.set_high();
            self.spi
                .write(data)
                .map(|_| ())
                .with_context(|| format!("failed writing ST7789 payload for 0x{cmd:02X}"))?;
        }
        Ok(())
    }

    pub fn draw_menu(&mut self, state: &MenuState) -> Result<()> {
        const VISIBLE_ROWS: usize = 11;
        
        let mut fb = Framebuffer::new(240, 240);
        fb.clear(0x0000);
        fb.draw_text(8, 8, "PIRATE SYNTH", 0xFFFF, 0x0000);

        for (index, line) in state.lines().iter().enumerate() {
            if index >= state.scroll_offset && index < state.scroll_offset + VISIBLE_ROWS {
                let visual_row = index - state.scroll_offset;
                let y = 28 + (visual_row as i32 * 18);
                let selected = index == state.selected_item;
                let bg = if selected { 0x07E0 } else { 0x0000 };
                let fg = if selected { 0x0000 } else { 0xFFFF };
                fb.fill_rect(4, y - 2, 232, 14, bg);
                fb.draw_text(8, y, line, fg, bg);
            }
        }

        self.command(0x2A, &[0x00, 0x00, 0x00, 0xEF])?;
        self.command(0x2B, &[0x00, 0x00, 0x00, 0xEF])?;
        self.dc.set_low();
        self.spi
            .write(&[0x2C])
            .map(|_| ())
            .context("failed writing ST7789 RAMWR command")?;
        self.dc.set_high();
        let bytes = fb.to_bytes();
        write_in_chunks(&bytes, SPI_FRAMEBUFFER_CHUNK_SIZE, |chunk| {
            self.spi
                .write(chunk)
                .map(|_| ())
                .context("failed writing ST7789 framebuffer chunk")
        })?;
        Ok(())
    }

    pub fn draw_menu_to_ppm(state: &MenuState, path: &Path) -> Result<()> {
        const VISIBLE_ROWS: usize = 11;
        
        let mut fb = Framebuffer::new(240, 240);
        fb.clear(0x0000);
        fb.draw_text(8, 8, "PIRATE SYNTH", 0xFFFF, 0x0000);
        for (index, line) in state.lines().iter().enumerate() {
            if index >= state.scroll_offset && index < state.scroll_offset + VISIBLE_ROWS {
                let visual_row = index - state.scroll_offset;
                let y = 28 + (visual_row as i32 * 18);
                let selected = index == state.selected_item;
                let bg = if selected { 0x07E0 } else { 0x0000 };
                let fg = if selected { 0x0000 } else { 0xFFFF };
                fb.fill_rect(4, y - 2, 232, 14, bg);
                fb.draw_text(8, y, line, fg, bg);
            }
        }
        fb.save_ppm(path)
    }
}

fn write_in_chunks<F>(bytes: &[u8], chunk_size: usize, mut write_chunk: F) -> Result<()>
where
    F: FnMut(&[u8]) -> Result<()>,
{
    for chunk in bytes.chunks(chunk_size) {
        write_chunk(chunk)?;
    }
    Ok(())
}

fn parse_spi_device(spi_path: &str) -> Result<(Bus, SlaveSelect)> {
    let device = spi_path.rsplit('/').next().unwrap_or(spi_path);
    let mut parts = device.split('.');
    let bus_name = parts
        .next()
        .with_context(|| format!("invalid SPI device path {spi_path}"))?;
    let chip_select = parts
        .next()
        .with_context(|| format!("invalid SPI device path {spi_path}"))?;
    if parts.next().is_some() {
        anyhow::bail!("invalid SPI device path {spi_path}");
    }

    let bus = match bus_name {
        "spidev0" => Bus::Spi0,
        "spidev1" => Bus::Spi1,
        "spidev2" => Bus::Spi2,
        "spidev3" => Bus::Spi3,
        "spidev4" => Bus::Spi4,
        "spidev5" => Bus::Spi5,
        "spidev6" => Bus::Spi6,
        _ => anyhow::bail!("unsupported SPI bus in {spi_path}; expected /dev/spidevN.M"),
    };

    let slave_select = match chip_select {
        "0" => SlaveSelect::Ss0,
        "1" => SlaveSelect::Ss1,
        "2" => SlaveSelect::Ss2,
        "3" => SlaveSelect::Ss3,
        _ => anyhow::bail!("unsupported SPI chip-select in {spi_path}; expected 0-3"),
    };

    Ok((bus, slave_select))
}

struct Framebuffer {
    width: usize,
    height: usize,
    pixels: Vec<u16>,
}

impl Framebuffer {
    fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            pixels: vec![0; width * height],
        }
    }

    fn clear(&mut self, color: u16) {
        self.pixels.fill(color);
    }

    fn fill_rect(&mut self, x: i32, y: i32, w: i32, h: i32, color: u16) {
        for yy in y.max(0)..(y + h).min(self.height as i32) {
            for xx in x.max(0)..(x + w).min(self.width as i32) {
                self.set_pixel(xx as usize, yy as usize, color);
            }
        }
    }

    fn draw_text(&mut self, x: i32, y: i32, text: &str, fg: u16, bg: u16) {
        for (idx, ch) in text.chars().enumerate() {
            self.draw_char(x + (idx as i32 * 8), y, ch, fg, bg);
        }
    }

    fn draw_char(&mut self, x: i32, y: i32, ch: char, fg: u16, bg: u16) {
        if let Some(glyph) = font8x8::BASIC_FONTS.get(ch) {
            for (row, bits) in glyph.iter().enumerate() {
                for col in 0..8 {
                    let color = if (bits >> col) & 1 == 1 { fg } else { bg };
                    let px = x + col;
                    let py = y + row as i32;
                    if px >= 0
                        && py >= 0
                        && (px as usize) < self.width
                        && (py as usize) < self.height
                    {
                        self.set_pixel(px as usize, py as usize, color);
                    }
                }
            }
        }
    }

    fn set_pixel(&mut self, x: usize, y: usize, color: u16) {
        self.pixels[y * self.width + x] = color;
    }

    fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(self.pixels.len() * 2);
        for px in &self.pixels {
            bytes.push((px >> 8) as u8);
            bytes.push((*px & 0xff) as u8);
        }
        bytes
    }

    fn save_ppm(&self, path: &Path) -> Result<()> {
        let mut out = Vec::with_capacity(self.pixels.len() * 3 + 32);
        out.extend_from_slice(format!("P6\n{} {}\n255\n", self.width, self.height).as_bytes());
        for px in &self.pixels {
            let r = ((px >> 11) & 0x1f) as u8;
            let g = ((px >> 5) & 0x3f) as u8;
            let b = (px & 0x1f) as u8;
            out.push((r << 3) | (r >> 2));
            out.push((g << 2) | (g >> 4));
            out.push((b << 3) | (b >> 2));
        }
        fs::write(path, out)
            .with_context(|| format!("failed writing screenshot {}", path.display()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_navigation_wraps() {
        let mut menu = MenuState::new(2, 0.0);
        menu.apply_button(Button::Up);
        assert_eq!(menu.selected_item, menu.total_items() - 1);
    }

    #[test]
    fn menu_has_four_items_only() {
        let menu = MenuState::new(2, 0.0);
        assert_eq!(menu.total_items(), 4);
        assert_eq!(menu.lines().len(), 4);
    }

    #[test]
    fn menu_scroll_offset_initialized_to_zero() {
        let menu = MenuState::new(2, 0.0);
        assert_eq!(menu.scroll_offset, 0);
    }

    #[test]
    fn menu_scroll_stays_zero_with_four_items() {
        let mut menu = MenuState::new(2, 0.0);
        for _ in 0..20 {
            menu.apply_button(Button::Down);
        }
        assert_eq!(menu.scroll_offset, 0);
    }

    #[test]
    fn menu_lines_contains_key_oct_cents_spread() {
        let mut menu = MenuState::new(2, 0.0);
        menu.key_index = 0;
        menu.octave = 3;
        menu.fine_tune_cents = 5.5;
        menu.stereo_spread = 25;
        let lines = menu.lines();
        assert_eq!(lines[0], "KEY: C");
        assert_eq!(lines[1], "OCT: 3");
        assert_eq!(lines[2], "CENTS: +5");
        assert_eq!(lines[3], "SPREAD: 25");
    }

    #[test]
    fn parse_spi_device_accepts_common_paths() {
        assert_eq!(
            parse_spi_device("/dev/spidev0.0").unwrap(),
            (Bus::Spi0, SlaveSelect::Ss0)
        );
        assert_eq!(
            parse_spi_device("spidev0.1").unwrap(),
            (Bus::Spi0, SlaveSelect::Ss1)
        );
    }

    #[test]
    fn write_in_chunks_preserves_order_and_boundaries() {
        let data = (0..10).collect::<Vec<u8>>();
        let mut writes = Vec::new();

        write_in_chunks(&data, 4, |chunk| {
            writes.push(chunk.to_vec());
            Ok(())
        })
        .unwrap();

        assert_eq!(writes, vec![vec![0, 1, 2, 3], vec![4, 5, 6, 7], vec![8, 9]]);
        assert_eq!(writes.concat(), data);
    }
}
