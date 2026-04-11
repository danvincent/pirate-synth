use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::Path;

use anyhow::{Context, Result};
use font8x8::UnicodeFonts;

const SPI_FRAMEBUFFER_CHUNK_SIZE: usize = 4096;

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
    pub wavetables: Vec<String>,
    pub selected_wavetable: usize,
    pub key_index: usize,
    pub octave: i32,
    pub fine_tune_cents: f32,
    pub selected_item: usize,
}

impl MenuState {
    pub fn with_wavetables(wavetables: Vec<String>, octave: i32, fine_tune_cents: f32) -> Self {
        Self {
            wavetables,
            selected_wavetable: 0,
            key_index: 0,
            octave,
            fine_tune_cents,
            selected_item: 0,
        }
    }

    pub fn key_name(&self) -> &'static str {
        KEY_NAMES[self.key_index]
    }

    pub fn total_items(&self) -> usize {
        3 + self.wavetables.len().max(1)
    }

    pub fn apply_button(&mut self, button: Button) {
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
    }

    fn increment_selected_value(&mut self) {
        match self.selected_item {
            0 => self.key_index = (self.key_index + 1) % KEY_NAMES.len(),
            1 => self.octave = (self.octave + 1).min(8),
            2 => self.fine_tune_cents = (self.fine_tune_cents + 1.0).min(100.0),
            index => {
                let wavetable_items = self.wavetables.len().max(1);
                self.selected_wavetable = (index - 3).min(wavetable_items - 1);
            }
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
            _ => {}
        }
    }

    pub fn lines(&self) -> Vec<String> {
        let mut lines = vec![
            format!("KEY: {}", self.key_name()),
            format!("OCT: {}", self.octave),
            format!("CENTS: {:+.0}", self.fine_tune_cents),
        ];

        if self.wavetables.is_empty() {
            lines.push("WT: (default sine)".to_string());
        } else {
            for (index, wavetable) in self.wavetables.iter().enumerate() {
                let prefix = if index == self.selected_wavetable {
                    "WT:*"
                } else {
                    "WT: "
                };
                lines.push(format!("{prefix}{wavetable}"));
            }
        }

        lines
    }
}

pub struct ButtonReader {
    pins: [SysfsPin; 4],
    last: [bool; 4],
}

impl ButtonReader {
    pub fn new() -> Result<Self> {
        let bcm = [5u32, 6, 16, 24];
        let mut pins = [
            SysfsPin::new(bcm[0]),
            SysfsPin::new(bcm[1]),
            SysfsPin::new(bcm[2]),
            SysfsPin::new(bcm[3]),
        ];
        for pin in &mut pins {
            pin.export_if_needed()?;
            pin.set_direction("in")?;
        }
        Ok(Self {
            pins,
            last: [false; 4],
        })
    }

    pub fn poll_pressed(&mut self) -> Result<Option<Button>> {
        let mapping = [Button::Up, Button::Down, Button::Select, Button::Back];
        for (idx, pin) in self.pins.iter_mut().enumerate() {
            let pressed = !pin.read_value()?;
            let rising = pressed && !self.last[idx];
            self.last[idx] = pressed;
            if rising {
                return Ok(Some(mapping[idx]));
            }
        }
        Ok(None)
    }
}

pub struct St7789Display {
    spi: File,
    dc: SysfsPin,
    backlight: Option<SysfsPin>,
}

impl St7789Display {
    pub fn new(spi_path: &str, dc_pin: u32, backlight_pin: Option<u32>) -> Result<Self> {
        let spi = File::options()
            .write(true)
            .open(spi_path)
            .with_context(|| format!("failed to open SPI device {spi_path}"))?;

        let mut dc = SysfsPin::new(dc_pin);
        dc.export_if_needed()?;
        dc.set_direction("out")?;

        let backlight = match backlight_pin {
            Some(pin) => {
                let mut p = SysfsPin::new(pin);
                p.export_if_needed()?;
                p.set_direction("out")?;
                p.write_value(true)?;
                Some(p)
            }
            None => None,
        };

        let mut display = Self { spi, dc, backlight };
        display.init()?;
        Ok(display)
    }

    fn init(&mut self) -> Result<()> {
        self.command(0x11, &[])?; // sleep out
        self.command(0x3A, &[0x55])?; // RGB565
        self.command(0x36, &[0x60])?; // rotation
        self.command(0x21, &[])?; // display inversion on
        self.command(0x29, &[])?; // display on
        if let Some(backlight) = &mut self.backlight {
            backlight.write_value(true)?;
        }
        Ok(())
    }

    fn command(&mut self, cmd: u8, data: &[u8]) -> Result<()> {
        self.dc.write_value(false)?;
        self.spi.write_all(&[cmd])?;
        if !data.is_empty() {
            self.dc.write_value(true)?;
            self.spi.write_all(data)?;
        }
        Ok(())
    }

    pub fn draw_menu(&mut self, state: &MenuState) -> Result<()> {
        let mut fb = Framebuffer::new(240, 240);
        fb.clear(0x0000);
        fb.draw_text(8, 8, "PIRATE SYNTH", 0xFFFF, 0x0000);

        for (index, line) in state.lines().iter().enumerate() {
            let y = 28 + (index as i32 * 18);
            let selected = index == state.selected_item;
            let bg = if selected { 0x07E0 } else { 0x0000 };
            let fg = if selected { 0x0000 } else { 0xFFFF };
            fb.fill_rect(4, y - 2, 232, 14, bg);
            fb.draw_text(8, y, line, fg, bg);
        }

        self.command(0x2A, &[0x00, 0x00, 0x00, 0xEF])?;
        self.command(0x2B, &[0x00, 0x00, 0x00, 0xEF])?;
        self.dc.write_value(false)?;
        self.spi.write_all(&[0x2C])?;
        self.dc.write_value(true)?;
        let bytes = fb.to_bytes();
        for chunk in bytes.chunks(SPI_FRAMEBUFFER_CHUNK_SIZE) {
            self.spi.write_all(chunk)?;
        }
        Ok(())
    }

    pub fn draw_menu_to_ppm(state: &MenuState, path: &Path) -> Result<()> {
        let mut fb = Framebuffer::new(240, 240);
        fb.clear(0x0000);
        fb.draw_text(8, 8, "PIRATE SYNTH", 0xFFFF, 0x0000);
        for (index, line) in state.lines().iter().enumerate() {
            let y = 28 + (index as i32 * 18);
            let selected = index == state.selected_item;
            let bg = if selected { 0x07E0 } else { 0x0000 };
            let fg = if selected { 0x0000 } else { 0xFFFF };
            fb.fill_rect(4, y - 2, 232, 14, bg);
            fb.draw_text(8, y, line, fg, bg);
        }
        fb.save_ppm(path)
    }
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

#[derive(Clone, Debug)]
struct SysfsPin {
    bcm_number: u32,
    sysfs_number: Option<u32>,
}

impl SysfsPin {
    fn new(number: u32) -> Self {
        Self {
            bcm_number: number,
            sysfs_number: None,
        }
    }

    fn number(&self) -> u32 {
        self.sysfs_number.unwrap_or(self.bcm_number)
    }

    fn path(&self) -> String {
        format!("/sys/class/gpio/gpio{}", self.number())
    }

    fn export_if_needed(&mut self) -> Result<()> {
        let sysfs_number = resolve_sysfs_gpio_number(self.bcm_number)?;
        self.sysfs_number = Some(sysfs_number);

        if !Path::new(&self.path()).exists() {
            fs::write("/sys/class/gpio/export", sysfs_number.to_string())
                .with_context(|| {
                    format!(
                        "failed to export bcm gpio{} as sysfs gpio{}",
                        self.bcm_number, sysfs_number
                    )
                })?;
        }
        Ok(())
    }

    fn set_direction(&mut self, direction: &str) -> Result<()> {
        fs::write(format!("{}/direction", self.path()), direction)
            .with_context(|| format!("failed to set gpio{} direction", self.number()))?;
        Ok(())
    }

    fn read_value(&mut self) -> Result<bool> {
        let mut value = String::new();
        File::open(format!("{}/value", self.path()))
            .with_context(|| format!("failed reading gpio{} value", self.number()))?
            .read_to_string(&mut value)?;
        Ok(value.trim() == "1")
    }

    fn write_value(&mut self, high: bool) -> Result<()> {
        fs::write(
            format!("{}/value", self.path()),
            if high { "1" } else { "0" },
        )
        .with_context(|| format!("failed writing gpio{} value", self.number()))?;
        Ok(())
    }
}

fn resolve_sysfs_gpio_number(bcm_number: u32) -> Result<u32> {
    let mut chips = Vec::new();
    for entry in fs::read_dir("/sys/class/gpio").context("failed to read /sys/class/gpio")? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with("gpiochip") {
            continue;
        }

        let chip_path = entry.path();
        let base = fs::read_to_string(chip_path.join("base"))
            .with_context(|| format!("failed reading {} base", chip_path.display()))?
            .trim()
            .parse::<u32>()
            .with_context(|| format!("failed parsing {} base", chip_path.display()))?;
        let ngpio = fs::read_to_string(chip_path.join("ngpio"))
            .with_context(|| format!("failed reading {} ngpio", chip_path.display()))?
            .trim()
            .parse::<u32>()
            .with_context(|| format!("failed parsing {} ngpio", chip_path.display()))?;
        let label = fs::read_to_string(chip_path.join("label"))
            .ok()
            .map(|value| value.trim().to_ascii_lowercase())
            .unwrap_or_default();
        chips.push((base, ngpio, label));
    }

    let base = select_gpio_chip_base(&chips, bcm_number);
    Ok(base.map_or(bcm_number, |gpio_base| gpio_base + bcm_number))
}

const SOC_GPIO_LABEL_HINTS: [&str; 3] = ["bcm", "pinctrl", "raspberrypi"];

fn select_gpio_chip_base(chips: &[(u32, u32, String)], bcm_number: u32) -> Option<u32> {
    let mut preferred = None;
    let mut fallback = None;

    for (base, ngpio, label) in chips {
        if *ngpio <= bcm_number {
            continue;
        }
        fallback = Some(fallback.map_or(*base, |current: u32| current.min(*base)));

        if SOC_GPIO_LABEL_HINTS
            .iter()
            .any(|hint| label.contains(hint))
        {
            preferred = Some(preferred.map_or(*base, |current: u32| current.min(*base)));
        }
    }

    preferred.or(fallback)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_navigation_wraps() {
        let mut menu = MenuState::with_wavetables(vec!["a".to_string()], 2, 0.0);
        menu.apply_button(Button::Up);
        assert_eq!(menu.selected_item, menu.total_items() - 1);
    }

    #[test]
    fn gpio_chip_base_prefers_bcm_like_labels() {
        let chips = vec![
            (0, 8, "other".to_string()),
            (512, 54, "pinctrl-bcm2835".to_string()),
        ];
        assert_eq!(select_gpio_chip_base(&chips, 5), Some(512));
    }

    #[test]
    fn gpio_chip_base_falls_back_to_first_matching_chip() {
        let chips = vec![(256, 32, "unknown".to_string()), (512, 54, "other".to_string())];
        assert_eq!(select_gpio_chip_base(&chips, 5), Some(256));
    }

    #[test]
    fn gpio_chip_base_fallback_is_order_independent() {
        let chips = vec![(512, 54, "other".to_string()), (256, 32, "unknown".to_string())];
        assert_eq!(select_gpio_chip_base(&chips, 5), Some(256));
    }
}
