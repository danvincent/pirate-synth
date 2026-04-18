use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use rppal::gpio::{Gpio, InputPin, OutputPin};
use rppal::spi::{Bus, Mode, SlaveSelect, Spi};

// Keep SPI transfers small to avoid EMSGSIZE from Linux spidev on constrained targets.
const SPI_FRAMEBUFFER_CHUNK_SIZE: usize = 4096;
const SPI_CLOCK_HZ: u32 = 16_000_000;

const FONT_DATA: [[u8; 8]; 128] = [
    [0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00],
    [0x7C,0x82,0xAA,0x82,0xBA,0x82,0x7C,0x00],
    [0x00,0x00,0x6C,0x92,0x92,0x6C,0x00,0x00],
    [0x44,0xEE,0xFE,0xFE,0x7C,0x38,0x10,0x00],
    [0x10,0x38,0x7C,0xFE,0x7C,0x38,0x10,0x00],
    [0x10,0x38,0x54,0xEE,0x54,0x10,0x38,0x00],
    [0x10,0x38,0x7C,0xEE,0x54,0x10,0x38,0x00],
    [0x00,0x00,0x18,0x3C,0x18,0x00,0x00,0x00],
    [0xF0,0xF0,0xF0,0xF0,0xF0,0xF0,0xF0,0xF0],
    [0x00,0x00,0x3C,0x66,0x66,0x3C,0x00,0x00],
    [0x0F,0x0F,0x0F,0x0F,0x0F,0x0F,0x0F,0x0F],
    [0x1E,0x0E,0x1A,0x38,0x6C,0x6C,0x38,0x00],
    [0x38,0x6C,0x6C,0x38,0x10,0x38,0x10,0x00],
    [0x10,0x18,0x14,0x14,0x10,0x70,0x60,0x00],
    [0x28,0x10,0x7C,0xC0,0x7C,0x06,0xFC,0x00],
    [0x10,0x54,0x38,0xC6,0x38,0x54,0x10,0x00],
    [0x80,0xE0,0xF8,0xFE,0xF8,0xE0,0x80,0x00],
    [0x02,0x0E,0x3E,0xFE,0x3E,0x0E,0x02,0x00],
    [0x10,0x38,0x7C,0x10,0x7C,0x38,0x10,0x00],
    [0x28,0x10,0x38,0x60,0x38,0x0C,0x78,0x00],
    [0x00,0x7E,0xB6,0xB6,0x76,0x36,0x36,0x00],
    [0x78,0xC0,0x78,0xCC,0x78,0x0C,0x78,0x00],
    [0x00,0x00,0x08,0x7C,0x10,0x7C,0x20,0x00],
    [0x28,0x10,0xFE,0x8C,0x30,0xC6,0xFE,0x00],
    [0x10,0x38,0x7C,0x10,0x10,0x10,0x10,0x00],
    [0x10,0x10,0x10,0x10,0x7C,0x38,0x10,0x00],
    [0x00,0x08,0x0C,0xFE,0x0C,0x08,0x00,0x00],
    [0x00,0x20,0x60,0xFE,0x60,0x20,0x00,0x00],
    [0x28,0x10,0x00,0x7C,0x18,0x30,0x7C,0x00],
    [0x00,0x28,0x6C,0xFE,0x6C,0x28,0x00,0x00],
    [0x10,0x10,0x38,0x38,0x7C,0x7C,0xFE,0x00],
    [0xFE,0x7C,0x7C,0x38,0x38,0x10,0x10,0x00],
    [0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00],
    [0x00,0x30,0x78,0x78,0x30,0x00,0x30,0x00],
    [0xC6,0xC6,0x6C,0x00,0x00,0x00,0x00,0x00],
    [0x00,0x6C,0xFE,0x6C,0x6C,0xFE,0x6C,0x00],
    [0x10,0x7C,0xD0,0x7C,0x16,0xD6,0x7C,0x10],
    [0x00,0x44,0xE8,0x50,0x28,0x5C,0x88,0x00],
    [0x00,0x30,0x58,0x72,0xDC,0xCC,0x7A,0x00],
    [0x18,0x18,0x30,0x00,0x00,0x00,0x00,0x00],
    [0x00,0x0C,0x18,0x18,0x18,0x18,0x0C,0x00],
    [0x00,0x60,0x30,0x30,0x30,0x30,0x60,0x00],
    [0x00,0x00,0x48,0x30,0xFC,0x30,0x48,0x00],
    [0x00,0x00,0x30,0x30,0xFC,0x30,0x30,0x00],
    [0x00,0x00,0x00,0x00,0x00,0x18,0x18,0x30],
    [0x00,0x00,0x00,0x00,0x7C,0x00,0x00,0x00],
    [0x00,0x00,0x00,0x00,0x00,0x00,0x30,0x00],
    [0x00,0x06,0x0C,0x18,0x30,0x60,0xC0,0x00],
    [0x00,0x7C,0xCE,0xD6,0xD6,0xE6,0x7C,0x00],
    [0x00,0x30,0x70,0xF0,0x30,0x30,0xFC,0x00],
    [0x00,0x7C,0xC6,0x06,0x7C,0xC0,0xFE,0x00],
    [0x00,0x7C,0xC6,0x1C,0x06,0xC6,0x7C,0x00],
    [0x00,0x3C,0x6C,0xCC,0xFE,0x0C,0x0C,0x00],
    [0x00,0xFE,0xC0,0xFC,0x06,0xC6,0x7C,0x00],
    [0x00,0x7C,0xC0,0xFC,0xC6,0xC6,0x7C,0x00],
    [0x00,0xFE,0x06,0x0C,0x0C,0x18,0x18,0x00],
    [0x00,0x7C,0xC6,0x7C,0xC6,0xC6,0x7C,0x00],
    [0x00,0x7C,0xC6,0xC6,0x7E,0x06,0x7C,0x00],
    [0x00,0x00,0x00,0x30,0x00,0x00,0x30,0x00],
    [0x00,0x00,0x00,0x30,0x00,0x30,0x30,0x60],
    [0x00,0x00,0x18,0x30,0x60,0x30,0x18,0x00],
    [0x00,0x00,0x00,0x7C,0x00,0x7C,0x00,0x00],
    [0x00,0x00,0x30,0x18,0x0C,0x18,0x30,0x00],
    [0x00,0x7C,0xC6,0x0C,0x18,0x00,0x18,0x00],
    [0x00,0x7C,0xCE,0xDA,0xCE,0xC0,0x7E,0x00],
    [0x00,0x10,0x38,0x6C,0x6C,0xFE,0xC6,0x00],
    [0x00,0xFC,0x66,0x7C,0x66,0x66,0xFC,0x00],
    [0x00,0x7C,0xC6,0xC0,0xC0,0xC6,0x7C,0x00],
    [0x00,0xFC,0x66,0x66,0x66,0x66,0xFC,0x00],
    [0x00,0xFE,0x62,0x78,0x60,0x62,0xFE,0x00],
    [0x00,0xFE,0x62,0x78,0x60,0x60,0xF0,0x00],
    [0x00,0x7C,0xC6,0xC0,0xCE,0xC6,0x7C,0x00],
    [0x00,0xC6,0xC6,0xFE,0xC6,0xC6,0xC6,0x00],
    [0x00,0xFC,0x30,0x30,0x30,0x30,0xFC,0x00],
    [0x00,0x1E,0x0C,0x0C,0x0C,0xCC,0x78,0x00],
    [0x00,0xE6,0x6C,0x78,0x6C,0x66,0xE6,0x00],
    [0x00,0xF0,0x60,0x60,0x60,0x64,0xFC,0x00],
    [0x00,0xC6,0xEE,0xFE,0xD6,0xC6,0xC6,0x00],
    [0x00,0xC6,0xE6,0xD6,0xD6,0xCE,0xC6,0x00],
    [0x00,0x7C,0xC6,0xC6,0xC6,0xC6,0x7C,0x00],
    [0x00,0xFC,0x66,0x66,0x7C,0x60,0xF0,0x00],
    [0x00,0x7C,0xC6,0xC6,0xC6,0xDE,0x7C,0x06],
    [0x00,0xFC,0x66,0x66,0x7C,0x66,0xF6,0x00],
    [0x00,0x7C,0xC0,0x7C,0x06,0xC6,0x7C,0x00],
    [0x00,0xFC,0xB4,0x30,0x30,0x30,0x78,0x00],
    [0x00,0xC6,0xC6,0xC6,0xC6,0xC6,0x7C,0x00],
    [0x00,0xC6,0xC6,0x6C,0x6C,0x38,0x10,0x00],
    [0x00,0xC6,0xC6,0xD6,0xFE,0xEE,0x44,0x00],
    [0x00,0xC6,0x6C,0x38,0x38,0x6C,0xC6,0x00],
    [0x00,0xCC,0xCC,0x78,0x30,0x30,0x78,0x00],
    [0x00,0xFE,0xCC,0x18,0x30,0x66,0xFE,0x00],
    [0x00,0x3E,0x30,0x30,0x30,0x30,0x3E,0x00],
    [0x00,0xC0,0x60,0x30,0x18,0x0C,0x06,0x00],
    [0x00,0xF8,0x18,0x18,0x18,0x18,0xF8,0x00],
    [0x38,0x6C,0x00,0x00,0x00,0x00,0x00,0x00],
    [0x00,0x00,0x00,0x00,0x00,0x00,0x00,0xFE],
    [0x30,0x18,0x00,0x00,0x00,0x00,0x00,0x00],
    [0x00,0x00,0x00,0x7A,0xCC,0xCC,0x76,0x00],
    [0x00,0xE0,0x60,0x7C,0x66,0x66,0xDC,0x00],
    [0x00,0x00,0x00,0x3C,0x60,0x60,0x3C,0x00],
    [0x00,0x1C,0x0C,0x7C,0xCC,0xCC,0x76,0x00],
    [0x00,0x00,0x00,0x78,0xCC,0xFC,0x60,0x30],
    [0x00,0x3C,0x66,0xF0,0x60,0x60,0xF0,0x00],
    [0x00,0x00,0x00,0x76,0xCC,0x7C,0x0C,0x78],
    [0x00,0xE0,0x60,0x6C,0x76,0x66,0xE6,0x00],
    [0x00,0x30,0x00,0x70,0x30,0x30,0x38,0x00],
    [0x00,0x18,0x00,0x38,0x18,0x18,0x18,0x70],
    [0x00,0xE0,0x60,0x6C,0x78,0x6C,0xE6,0x00],
    [0x00,0x70,0x30,0x30,0x30,0x30,0x38,0x00],
    [0x00,0x00,0x00,0xEC,0xD6,0xD6,0xC6,0x00],
    [0x00,0x00,0x00,0xDC,0x76,0x66,0x66,0x00],
    [0x00,0x00,0x00,0x78,0xCC,0xCC,0x78,0x00],
    [0x00,0x00,0x00,0xDC,0x66,0x7C,0x60,0xF0],
    [0x00,0x00,0x00,0x76,0xCC,0x7C,0x0C,0x1E],
    [0x00,0x00,0x00,0xDC,0x66,0x60,0xF0,0x00],
    [0x00,0x00,0x38,0x60,0x38,0x0C,0x78,0x00],
    [0x00,0x00,0x60,0xF8,0x60,0x6C,0x38,0x00],
    [0x00,0x00,0x00,0xCC,0xCC,0xCC,0x76,0x00],
    [0x00,0x00,0x00,0xC6,0x6C,0x38,0x10,0x00],
    [0x00,0x00,0x00,0xC6,0xD6,0xD6,0x6C,0x00],
    [0x00,0x00,0x00,0xCC,0x78,0x78,0xCC,0x00],
    [0x00,0x00,0x00,0xCC,0xCC,0x7C,0x0C,0x78],
    [0x00,0x00,0x00,0x7C,0x18,0x30,0x7C,0x00],
    [0x00,0x1C,0x30,0xE0,0x30,0x30,0x1C,0x00],
    [0x00,0x30,0x30,0x30,0x30,0x30,0x30,0x00],
    [0x00,0x70,0x18,0x0E,0x18,0x18,0x70,0x00],
    [0x76,0xDC,0x00,0x00,0x00,0x00,0x00,0x00],
    [0x00,0x10,0x38,0x6C,0x6C,0xC6,0xFE,0x00],
];

pub const KEY_NAMES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];

pub const SCALE_NAMES: [&str; 9] = [
    "N/A",
    "Major",
    "Minor",
    "Penta",
    "Dorian",
    "Mixo",
    "Whole",
    "Hirajoshi",
    "Lydian",
];

pub const BANK_NAMES: [&str; 4] = ["A", "B", "C", "D"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Up,
    Down,
    Select,
    Back,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VideoStatus {
    Off,
    On,
    NoHdmi,
}

impl VideoStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::On => "On",
            Self::NoHdmi => "No HDMI",
        }
    }
}

#[derive(Clone, Debug)]
pub struct MenuState {
    pub key_index: usize,
    pub octave: i32,
    pub fine_tune_cents: f32,
    pub stereo_spread: u8,
    pub scale_index: usize,
    pub bank_index: usize,
    pub wt_volume: u8,
    pub gr_volume: u8,
    pub oscillators_active: bool,
    pub granular_active: bool,
    pub osc_count: usize,
    pub gr_voices: usize,
    pub video_status: VideoStatus,
    pub glide_progress: Option<f32>,
    pub selected_item: usize,
    pub scroll_offset: usize,
}

impl MenuState {
    pub fn new(fine_tune_cents: f32, osc_count: usize, gr_voices: usize) -> Self {
        Self {
            key_index: 9,
            octave: 1,
            fine_tune_cents,
            stereo_spread: 100,
            scale_index: 7,
            bank_index: 0,
            wt_volume: 50,
            gr_volume: 50,
            oscillators_active: false,
            granular_active: false,
            osc_count,
            gr_voices,
            video_status: VideoStatus::Off,
            glide_progress: None,
            selected_item: 0,
            scroll_offset: 0,
        }
    }

    pub fn key_name(&self) -> &'static str {
        KEY_NAMES[self.key_index]
    }

    pub fn total_items(&self) -> usize {
        14
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
            0 => self.oscillators_active = !self.oscillators_active,
            1 => self.granular_active = !self.granular_active,
            2 => self.key_index = (self.key_index + 1) % KEY_NAMES.len(),
            3 => self.scale_index = (self.scale_index + 1) % SCALE_NAMES.len(),
            4 => self.octave = (self.octave + 1).min(8),
            5 => self.stereo_spread = (self.stereo_spread + 5).min(100),
            6 => self.bank_index = (self.bank_index + 1) % BANK_NAMES.len(),
            7 => self.wt_volume = (self.wt_volume + 10).min(100),
            8 => self.gr_volume = (self.gr_volume + 10).min(100),
            9 => self.fine_tune_cents = (self.fine_tune_cents + 1.0).min(100.0),
            10 => self.osc_count = (self.osc_count + 1).min(64),
            11 => self.gr_voices = (self.gr_voices + 1).min(64),
            _ => {}
        }
    }

    fn decrement_selected_value(&mut self) {
        match self.selected_item {
            0 => self.oscillators_active = !self.oscillators_active,
            1 => self.granular_active = !self.granular_active,
            2 => {
                if self.key_index == 0 {
                    self.key_index = KEY_NAMES.len() - 1;
                } else {
                    self.key_index -= 1;
                }
            }
            3 => {
                if self.scale_index == 0 {
                    self.scale_index = SCALE_NAMES.len() - 1;
                } else {
                    self.scale_index -= 1;
                }
            }
            4 => self.octave = (self.octave - 1).max(0),
            5 => self.stereo_spread = self.stereo_spread.saturating_sub(5),
            6 => {
                if self.bank_index == 0 {
                    self.bank_index = BANK_NAMES.len() - 1;
                } else {
                    self.bank_index -= 1;
                }
            }
            7 => self.wt_volume = self.wt_volume.saturating_sub(10),
            8 => self.gr_volume = self.gr_volume.saturating_sub(10),
            9 => self.fine_tune_cents = (self.fine_tune_cents - 1.0).max(-100.0),
            10 => self.osc_count = (self.osc_count - 1).max(1),
            11 => self.gr_voices = self.gr_voices.saturating_sub(1),
            _ => {}
        }
    }

    pub fn lines(&self) -> Vec<String> {
        vec![
            format!("Wavetable: {}", if self.oscillators_active { "On" } else { "Off" }),
            format!("Granular: {}", if self.granular_active { "On" } else { "Off" }),
            format!("Key: {}", self.key_name()),
            format!("Scale: {}", SCALE_NAMES[self.scale_index]),
            format!("Octave: {}", self.octave),
            format!("Stereo: {}", self.stereo_spread),
            format!("WT Bank: {}", BANK_NAMES[self.bank_index]),
            format!("WT Vol: {}", self.wt_volume),
            format!("GR Vol: {}", self.gr_volume),
            format!("Cents: {:+}", self.fine_tune_cents as i32),
            format!("WT Oscs: {}", self.osc_count),
            format!("GR Voices: {}", self.gr_voices),
            format!("Video: {}", self.video_status.as_str()),
            format!("Glide: {}", match self.glide_progress {
                None => "---".to_string(),
                Some(p) => format!("{}%", (p * 100.0) as u32),
            }),
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
        self.command(0x01, &[])?; // SWRESET
        std::thread::sleep(std::time::Duration::from_millis(150));
        self.command(0x11, &[])?; // SLPOUT
        std::thread::sleep(std::time::Duration::from_millis(10));
        self.command(0x3A, &[0x55])?; // COLMOD RGB565
        self.command(0x36, &[0x00])?; // MADCTL default orientation
        self.command(0x21, &[])?; // INVON
        self.command(0x13, &[])?; // NORON
        self.command(0x29, &[])?; // DISPON
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
        fb.draw_text(8, 8, "Pirate Synth", 0xFFFF, 0x0000);

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
        fb.draw_text(8, 8, "Pirate Synth", 0xFFFF, 0x0000);
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

    pub fn draw_idle_screen(&mut self, state: &MenuState) -> Result<()> {
        let tmp = std::path::PathBuf::from("/tmp/pirate-synth-idle.ppm");
        // Render to a temporary PPM first (reuses idle renderer), then convert to framebuffer.
        // For now, render directly using the same logic.
        let mut fb = Framebuffer::new(240, 240);
        fb.clear(0x0000);

        let key = state.key_name();
        let key_chars = key.chars().count();
        let key_total_w = key_chars as i32 * 32;
        let key_x = (240 - key_total_w) / 2;
        fb.draw_text_4x(key_x, 10, key, 0xFFFF, 0x0000);

        let octave_str = format!("{}", state.octave);
        let octave_x = key_x + key_total_w + 4;
        fb.draw_text_2x(octave_x, 28, &octave_str, 0x07E0, 0x0000);

        let scale = SCALE_NAMES[state.scale_index];
        let scale_w = scale.chars().count() as i32 * 16;
        let scale_x = (240 - scale_w) / 2;
        fb.draw_text_2x(scale_x, 58, scale, 0xAD55, 0x0000);

        let bar_max_h = 80i32;
        let bar_w = 50i32;
        let bar_top = 90i32;

        let wt_color: u16 = if state.oscillators_active { 0x07E0 } else { 0x2945 };
        let wt_bar_h = (bar_max_h * state.wt_volume as i32 / 100).max(2);
        let wt_x = 35i32;
        fb.fill_rect(wt_x, bar_top, bar_w, bar_max_h, 0x1084);
        fb.fill_rect(wt_x, bar_top + (bar_max_h - wt_bar_h), bar_w, wt_bar_h, wt_color);
        fb.draw_text_2x(wt_x + 9, bar_top + bar_max_h + 4, "WT", if state.oscillators_active { 0x07E0 } else { 0x8410 }, 0x0000);
        let wt_vol_str = format!("{:3}", state.wt_volume);
        fb.draw_text(wt_x + 9, bar_top + bar_max_h + 22, &wt_vol_str, 0xFFFF, 0x0000);

        let gr_color: u16 = if state.granular_active { 0x001F } else { 0x2945 };
        let gr_bar_h = (bar_max_h * state.gr_volume as i32 / 100).max(2);
        let gr_x = 155i32;
        fb.fill_rect(gr_x, bar_top, bar_w, bar_max_h, 0x1084);
        fb.fill_rect(gr_x, bar_top + (bar_max_h - gr_bar_h), bar_w, gr_bar_h, gr_color);
        fb.draw_text_2x(gr_x + 9, bar_top + bar_max_h + 4, "GR", if state.granular_active { 0x001F } else { 0x8410 }, 0x0000);
        let gr_vol_str = format!("{:3}", state.gr_volume);
        fb.draw_text(gr_x + 9, bar_top + bar_max_h + 22, &gr_vol_str, 0xFFFF, 0x0000);

        let wave_color: u16 = if state.oscillators_active || state.granular_active { 0x4208 } else { 0x2104 };
        let wave_y_center = 195i32;
        for x in 0..240usize {
            let t = x as f32 * std::f32::consts::TAU / 240.0 * 2.5;
            let y_off = (t.sin() * 14.0) as i32;
            let y_px = wave_y_center + y_off;
            if y_px >= 0 && y_px < 239 {
                fb.set_pixel(x, y_px as usize, wave_color);
                fb.set_pixel(x, (y_px + 1) as usize, wave_color);
            }
        }

        fb.draw_text(44, 226, "Press any key", 0x4208, 0x0000);

        let _ = tmp; // suppress unused warning
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
                .context("failed writing ST7789 idle screen framebuffer chunk")
        })?;
        Ok(())
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
        let idx = ch as usize;
        if idx < FONT_DATA.len() {
            let glyph = &FONT_DATA[idx];
            for (row, bits) in glyph.iter().enumerate() {
                for col in 0..8 {
                    let color = if (bits >> (7 - col)) & 1 == 1 { fg } else { bg };
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

    fn draw_char_2x(&mut self, x: i32, y: i32, ch: char, fg: u16, bg: u16) {
        let idx = ch as usize;
        if idx < FONT_DATA.len() {
            let glyph = &FONT_DATA[idx];
            for (row, bits) in glyph.iter().enumerate() {
                for col in 0..8i32 {
                    let color = if (bits >> (7 - col)) & 1 == 1 { fg } else { bg };
                    let px = x + col * 2;
                    let py = y + row as i32 * 2;
                    for dy in 0..2i32 {
                        for dx in 0..2i32 {
                            let fpx = px + dx;
                            let fpy = py + dy;
                            if fpx >= 0
                                && fpy >= 0
                                && (fpx as usize) < self.width
                                && (fpy as usize) < self.height
                            {
                                self.set_pixel(fpx as usize, fpy as usize, color);
                            }
                        }
                    }
                }
            }
        }
    }

    fn draw_text_2x(&mut self, x: i32, y: i32, text: &str, fg: u16, bg: u16) {
        for (idx, ch) in text.chars().enumerate() {
            self.draw_char_2x(x + (idx as i32 * 16), y, ch, fg, bg);
        }
    }

    fn draw_char_4x(&mut self, x: i32, y: i32, ch: char, fg: u16, bg: u16) {
        let idx = ch as usize;
        if idx < FONT_DATA.len() {
            let glyph = &FONT_DATA[idx];
            for (row, bits) in glyph.iter().enumerate() {
                for col in 0..8i32 {
                    let color = if (bits >> (7 - col)) & 1 == 1 { fg } else { bg };
                    let px = x + col * 4;
                    let py = y + row as i32 * 4;
                    for dy in 0..4i32 {
                        for dx in 0..4i32 {
                            let fpx = px + dx;
                            let fpy = py + dy;
                            if fpx >= 0
                                && fpy >= 0
                                && (fpx as usize) < self.width
                                && (fpy as usize) < self.height
                            {
                                self.set_pixel(fpx as usize, fpy as usize, color);
                            }
                        }
                    }
                }
            }
        }
    }

    fn draw_text_4x(&mut self, x: i32, y: i32, text: &str, fg: u16, bg: u16) {
        for (idx, ch) in text.chars().enumerate() {
            self.draw_char_4x(x + (idx as i32 * 32), y, ch, fg, bg);
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

/// Render two mockup frames showing the proposed UI redesign to PPM files in `dir`.
/// Screen 1: top of list, item 0 selected.
/// Screen 2: scrolled down, item 10 (WT OSCS) selected.
pub fn draw_redesign_mockups_ppm(dir: &std::path::Path) -> Result<()> {
    let state = MenuState {
        key_index: 9,          // A
        octave: 1,
        fine_tune_cents: 0.0,
        stereo_spread: 100,
        scale_index: 7,        // HIRAJOSHI
        bank_index: 0,
        wt_volume: 50,
        gr_volume: 50,
        oscillators_active: true,
        granular_active: false,
        osc_count: 8,
        gr_voices: 8,
        video_status: VideoStatus::Off,
        glide_progress: None,
        selected_item: 0,
        scroll_offset: 0,
    };

    // Screen 1: cursor at item 0, no scroll
    render_redesign_frame_to_ppm(
        &state,
        0,
        0,
        &dir.join("redesign-screen1-top.ppm"),
    )?;

    // Screen 2: cursor at item 10 (WT OSCS), scrolled (scroll_offset=1)
    render_redesign_frame_to_ppm(
        &state,
        10,
        1,
        &dir.join("redesign-screen2-scrolled.ppm"),
    )?;

    // Screen 3: idle overview screen
    render_idle_screen_to_ppm(
        &state,
        &dir.join("redesign-screen3-idle.ppm"),
    )?;

    Ok(())
}

fn render_redesign_frame_to_ppm(
    state: &MenuState,
    selected_item: usize,
    scroll_offset: usize,
    path: &std::path::Path,
) -> Result<()> {
    const VISIBLE_ROWS: usize = 11;

    let mut fb = Framebuffer::new(240, 240);
    fb.clear(0x0000);

    // ── Graphical header (no title, 26px) ─────────────────────────────────────
    // Left block: WT status (green if ON, dark if OFF)
    let wt_bg: u16 = if state.oscillators_active { 0x07E0 } else { 0x2945 };
    let wt_fg: u16 = if state.oscillators_active { 0x0000 } else { 0xFFFF };
    fb.fill_rect(0, 0, 116, 13, wt_bg);
    fb.draw_text(3, 3, "WT", wt_fg, wt_bg);
    let wt_state_str = if state.oscillators_active { "On " } else { "Off" };
    fb.draw_text(22, 3, wt_state_str, wt_fg, wt_bg);
    let wt_vol_str = format!("{:3}", state.wt_volume);
    fb.draw_text(88, 3, &wt_vol_str, wt_fg, wt_bg);
    // Volume bar (inside block, y=9, 2px tall)
    let wt_bar_w = (70i32 * state.wt_volume as i32 / 100).max(1);
    fb.fill_rect(48, 9, 70, 2, 0x0000); // track
    fb.fill_rect(48, 9, wt_bar_w, 2, if state.oscillators_active { 0xFFFF } else { 0x8410 });

    // Gap between blocks
    fb.fill_rect(116, 0, 8, 13, 0x0000);

    // Right block: GR status
    let gr_bg: u16 = if state.granular_active { 0x001F } else { 0x2945 }; // blue if ON
    let gr_fg: u16 = 0xFFFF;
    fb.fill_rect(124, 0, 116, 13, gr_bg);
    fb.draw_text(127, 3, "GR", gr_fg, gr_bg);
    let gr_state_str = if state.granular_active { "On " } else { "Off" };
    fb.draw_text(146, 3, gr_state_str, gr_fg, gr_bg);
    let gr_vol_str = format!("{:3}", state.gr_volume);
    fb.draw_text(212, 3, &gr_vol_str, gr_fg, gr_bg);
    let gr_bar_w = (70i32 * state.gr_volume as i32 / 100).max(1);
    fb.fill_rect(172, 9, 70, 2, 0x0000);
    fb.fill_rect(172, 9, gr_bar_w, 2, if state.granular_active { 0xFFFF } else { 0x8410 });

    // ── Key + Scale status line ───────────────────────────────────────────────
    let key_octave = format!("{}{}", state.key_name(), state.octave);
    let scale_name = SCALE_NAMES[state.scale_index];
    let status = format!("{} {}", key_octave, scale_name);
    fb.draw_text(4, 16, &status, 0xFFFF, 0x0000);

    // ── Divider ───────────────────────────────────────────────────────────────
    fb.fill_rect(0, 26, 240, 2, 0x4208);

    // ── Items ─────────────────────────────────────────────────────────────────
    let lines: Vec<String> = vec![
        format!("{:<20}{:>9}", "Wavetable", if state.oscillators_active { "On" } else { "Off" }),
        format!("{:<20}{:>9}", "Granular",  if state.granular_active    { "On" } else { "Off" }),
        format!("{:<20}{:>9}", "Key",       state.key_name()),
        format!("{:<20}{:>9}", "Octave",    state.octave),
        format!("{:<20}{:>9}", "Scale",     SCALE_NAMES[state.scale_index]),
        format!("{:<20}{:>9}", "WT Bank",   BANK_NAMES[state.bank_index]),
        format!("{:<20}{:>9}", "WT Vol",    state.wt_volume),
        format!("{:<20}{:>9}", "GR Vol",    state.gr_volume),
        format!("{:<20}{:>9}", "Stereo",    state.stereo_spread),
        format!("{:<20}{:>9}", "Cents",     format!("{:+}", state.fine_tune_cents as i32)),
        format!("{:<20}{:>9}", "WT Oscs",   state.osc_count),
        format!("{:<20}{:>9}", "GR Voices", state.gr_voices),
        format!("{:<20}{:>9}", "Video",     state.video_status.as_str()),
    ];

    for (index, line) in lines.iter().enumerate() {
        if index >= scroll_offset && index < scroll_offset + VISIBLE_ROWS {
            let visual_row = index - scroll_offset;
            let y = 30 + (visual_row as i32 * 18);
            let selected = index == selected_item;
            let bg = if selected { 0x07E0 } else { 0x0000 };
            let fg = if selected { 0x0000 } else { 0xFFFF };
            fb.fill_rect(2, y - 2, 236, 14, bg);
            fb.draw_text(4, y, line, fg, bg);
        }
    }

    fb.save_ppm(path)
}

/// Render the idle "at-a-glance" overview screen to a PPM file.
pub fn draw_idle_screen_to_ppm(state: &MenuState, path: &std::path::Path) -> Result<()> {
    render_idle_screen_to_ppm(state, path)
}

fn render_idle_screen_to_ppm(state: &MenuState, path: &std::path::Path) -> Result<()> {
    let mut fb = Framebuffer::new(240, 240);
    fb.clear(0x0000);

    // ── Large key + octave ────────────────────────────────────────────────────
    // Key at 4× (32px per char), centred
    let key = state.key_name(); // e.g. "A", "C#"
    let key_chars = key.chars().count();
    let key_total_w = key_chars as i32 * 32;
    let key_x = (240 - key_total_w) / 2;
    fb.draw_text_4x(key_x, 10, key, 0xFFFF, 0x0000);

    // Octave at 2× right of key
    let octave_str = format!("{}", state.octave);
    let octave_x = key_x + key_total_w + 4;
    fb.draw_text_2x(octave_x, 28, &octave_str, 0x07E0, 0x0000); // green octave

    // ── Scale name at 2× centred ──────────────────────────────────────────────
    let scale = SCALE_NAMES[state.scale_index];
    let scale_w = scale.chars().count() as i32 * 16;
    let scale_x = (240 - scale_w) / 2;
    fb.draw_text_2x(scale_x, 58, scale, 0xAD55, 0x0000); // muted cyan/teal

    // ── Volume bars ───────────────────────────────────────────────────────────
    // Each bar: 50px wide, up to 80px tall, vertical, fills from bottom
    let bar_max_h = 80i32;
    let bar_w = 50i32;
    let bar_top = 90i32; // top of bar area

    // WT bar (left side)
    let wt_color: u16 = if state.oscillators_active { 0x07E0 } else { 0x2945 };
    let wt_bar_h = (bar_max_h * state.wt_volume as i32 / 100).max(2);
    let wt_x = 35i32;
    fb.fill_rect(wt_x, bar_top, bar_w, bar_max_h, 0x1084); // track
    fb.fill_rect(wt_x, bar_top + (bar_max_h - wt_bar_h), bar_w, wt_bar_h, wt_color);
    // Label
    fb.draw_text_2x(wt_x + 9, bar_top + bar_max_h + 4, "WT", if state.oscillators_active { 0x07E0 } else { 0x8410 }, 0x0000);
    // Volume value
    let wt_vol_str = format!("{:3}", state.wt_volume);
    fb.draw_text(wt_x + 9, bar_top + bar_max_h + 22, &wt_vol_str, 0xFFFF, 0x0000);

    // GR bar (right side)
    let gr_color: u16 = if state.granular_active { 0x001F } else { 0x2945 }; // blue if ON
    let gr_bar_h = (bar_max_h * state.gr_volume as i32 / 100).max(2);
    let gr_x = 155i32;
    fb.fill_rect(gr_x, bar_top, bar_w, bar_max_h, 0x1084);
    fb.fill_rect(gr_x, bar_top + (bar_max_h - gr_bar_h), bar_w, gr_bar_h, gr_color);
    fb.draw_text_2x(gr_x + 9, bar_top + bar_max_h + 4, "GR", if state.granular_active { 0x001F } else { 0x8410 }, 0x0000);
    let gr_vol_str = format!("{:3}", state.gr_volume);
    fb.draw_text(gr_x + 9, bar_top + bar_max_h + 22, &gr_vol_str, 0xFFFF, 0x0000);

    // ── Sine wave ─────────────────────────────────────────────────────────────
    let wave_color: u16 = if state.oscillators_active || state.granular_active { 0x4208 } else { 0x2104 };
    let wave_y_center = 195i32;
    for x in 0..240usize {
        let t = x as f32 * std::f32::consts::TAU / 240.0 * 2.5;
        let y_off = (t.sin() * 14.0) as i32;
        let y_px = wave_y_center + y_off;
        if y_px >= 0 && y_px < 239 {
            fb.set_pixel(x, y_px as usize, wave_color);
            fb.set_pixel(x, (y_px + 1) as usize, wave_color);
        }
    }

    // ── "Press any key" ───────────────────────────────────────────────────────
    fb.draw_text(44, 226, "Press any key", 0x4208, 0x0000); // dim white

    fb.save_ppm(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_navigation_wraps() {
        let mut menu = MenuState::new(0.0, 8, 8);
        menu.apply_button(Button::Up);
        assert_eq!(menu.selected_item, menu.total_items() - 1);
    }

    #[test]
    fn menu_has_fourteen_items() {
        let menu = MenuState::new(0.0, 8, 8);
        assert_eq!(menu.total_items(), 14);
        assert_eq!(menu.lines().len(), 14);
    }

    #[test]
    fn menu_scroll_offset_initialized_to_zero() {
        let menu = MenuState::new(0.0, 8, 8);
        assert_eq!(menu.scroll_offset, 0);
    }

    #[test]
    fn menu_scroll_shifts_beyond_eleven_rows() {
        // 12 items with 11 visible rows; scrolling past item 10 should increase scroll_offset
        let mut menu = MenuState::new(0.0, 8, 8);
        for _ in 0..11 {
            menu.apply_button(Button::Down);
        }
        assert_eq!(menu.selected_item, 11);
        assert!(menu.scroll_offset > 0, "scroll_offset should shift when selected item exceeds visible window");
    }

    #[test]
    fn menu_default_values_match_spec() {
        let menu = MenuState::new(0.0, 8, 8);
        assert_eq!(menu.key_index, 9);           // A
        assert_eq!(menu.octave, 1);
        assert_eq!(menu.scale_index, 7);         // HIRAJOSHI
        assert_eq!(menu.stereo_spread, 100);
        assert_eq!(menu.wt_volume, 50);
        assert_eq!(menu.gr_volume, 50);
        assert_eq!(menu.oscillators_active, false);
        assert_eq!(menu.granular_active, false);
        assert_eq!(menu.fine_tune_cents, 0.0);
    }

    #[test]
    fn menu_lines_correct_labels() {
        let menu = MenuState::new(0.0, 8, 8);
        let lines = menu.lines();
        assert!(lines[0].starts_with("Wavetable:"), "line 0 should start with Wavetable:");
        assert!(lines[1].starts_with("Granular:"), "line 1 should start with Granular:");
        assert!(lines[2].starts_with("Key:"), "line 2 should start with Key:");
        assert!(lines[3].starts_with("Scale:"), "line 3 should start with Scale:");
        assert!(lines[4].starts_with("Octave:"), "line 4 should start with Octave:");
        assert!(lines[5].starts_with("Stereo:"), "line 5 should start with Stereo:");
        assert!(lines[6].starts_with("WT Bank:"), "line 6 should start with WT Bank:");
        assert!(lines[7].starts_with("WT Vol:"), "line 7 should start with WT Vol:");
        assert!(lines[8].starts_with("GR Vol:"), "line 8 should start with GR Vol:");
        assert!(lines[9].starts_with("Cents:"), "line 9 should start with Cents:");
        assert!(lines[10].starts_with("WT Oscs:"), "line 10 should start with WT Oscs:");
        assert!(lines[11].starts_with("GR Voices:"), "line 11 should start with GR Voices:");
        assert!(lines[12].starts_with("Video:"), "line 12 should start with Video:");
    }

    #[test]
    fn menu_video_line_reports_state() {
        let mut menu = MenuState::new(0.0, 8, 8);
        assert_eq!(menu.lines()[12], "Video: Off");
        menu.video_status = VideoStatus::On;
        assert_eq!(menu.lines()[12], "Video: On");
    }

    #[test]
    fn menu_glide_item_none_shows_dashes() {
        let mut menu = MenuState::new(0.0, 4, 2);
        menu.glide_progress = None;
        let lines = menu.lines();
        assert!(lines.iter().any(|l| l.contains("Glide: ---")));
    }

    #[test]
    fn menu_glide_item_some_shows_percent() {
        let mut menu = MenuState::new(0.0, 4, 2);
        menu.glide_progress = Some(0.5);
        let lines = menu.lines();
        assert!(lines.iter().any(|l| l.contains("Glide: 50%")));
    }

    #[test]
    fn glide_item_is_read_only() {
        let mut menu = MenuState::new(0.0, 4, 2);
        menu.selected_item = 13; // GLIDE item
        let before = menu.lines();
        menu.apply_button(Button::Select);
        menu.apply_button(Button::Back);
        assert_eq!(menu.lines(), before, "GLIDE item should be read-only");
    }

    #[test]
    fn menu_total_items_is_14() {
        let mut menu = MenuState::new(0.0, 4, 2);
        assert_eq!(menu.total_items(), 14);
        menu.video_status = VideoStatus::NoHdmi;
        assert_eq!(menu.lines()[12], "Video: No HDMI");
    }

    #[test]
    fn granular_toggle_activates() {
        let mut menu = MenuState::new(0.0, 8, 8);
        assert_eq!(menu.granular_active, false);
        menu.selected_item = 1;
        menu.apply_button(Button::Select);
        assert_eq!(menu.granular_active, true);
        menu.apply_button(Button::Select);
        assert_eq!(menu.granular_active, false);
    }

    #[test]
    fn wavetable_toggle_activates() {
        let mut menu = MenuState::new(0.0, 8, 8);
        assert_eq!(menu.oscillators_active, false);
        menu.selected_item = 0;
        menu.apply_button(Button::Select);
        assert_eq!(menu.oscillators_active, true);
        menu.apply_button(Button::Back);
        assert_eq!(menu.oscillators_active, false);
    }

    #[test]
    fn wt_volume_increments_by_ten() {
        let mut menu = MenuState::new(0.0, 8, 8);
        assert_eq!(menu.wt_volume, 50);
        menu.selected_item = 7;
        menu.apply_button(Button::Select);
        assert_eq!(menu.wt_volume, 60);
        menu.apply_button(Button::Back);
        assert_eq!(menu.wt_volume, 50);
    }

    #[test]
    fn gr_volume_increments_by_ten() {
        let mut menu = MenuState::new(0.0, 8, 8);
        assert_eq!(menu.gr_volume, 50);
        menu.selected_item = 8;
        menu.apply_button(Button::Select);
        assert_eq!(menu.gr_volume, 60);
        menu.apply_button(Button::Back);
        assert_eq!(menu.gr_volume, 50);
    }

    #[test]
    fn osc_count_increments_by_one_clamped() {
        let mut menu = MenuState::new(0.0, 64, 8);
        menu.selected_item = 10;
        menu.apply_button(Button::Select);
        assert_eq!(menu.osc_count, 64, "osc_count should clamp at 64");
        let mut menu2 = MenuState::new(0.0, 1, 8);
        menu2.selected_item = 10;
        menu2.apply_button(Button::Back);
        assert_eq!(menu2.osc_count, 1, "osc_count should clamp at minimum 1");
    }

    #[test]
    fn gr_voices_increments_by_one_clamped() {
        let mut menu = MenuState::new(0.0, 8, 64);
        menu.selected_item = 11;
        menu.apply_button(Button::Select);
        assert_eq!(menu.gr_voices, 64, "gr_voices should clamp at 64");
        let mut menu2 = MenuState::new(0.0, 8, 0);
        menu2.selected_item = 11;
        menu2.apply_button(Button::Back);
        assert_eq!(menu2.gr_voices, 0, "gr_voices should allow zero");
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
