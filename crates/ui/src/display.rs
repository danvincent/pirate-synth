use anyhow::{Context, Result};
use rppal::gpio::OutputPin;
use rppal::spi::{Bus, Mode, SlaveSelect, Spi};
use std::path::Path;
use crate::framebuffer::Framebuffer;
use crate::menu::{MenuState, SCALE_NAMES};

// Keep SPI transfers small to avoid EMSGSIZE from Linux spidev on constrained targets.
pub(crate) const SPI_FRAMEBUFFER_CHUNK_SIZE: usize = 4096;
pub(crate) const SPI_CLOCK_HZ: u32 = 16_000_000;

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

        let gpio = rppal::gpio::Gpio::new().context("failed to open GPIO controller")?;
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
        let selected_item = state.selected_item;
        let scroll_offset = state.scroll_offset;

        let mut fb = Framebuffer::new(240, 240);
        fb.clear(0x0000);

        let wt_bg: u16 = if state.oscillators_active { 0x07E0 } else { 0x2945 };
        let wt_fg: u16 = if state.oscillators_active { 0x0000 } else { 0xFFFF };
        fb.fill_rect(0, 0, 116, 13, wt_bg);
        fb.draw_text(3, 3, "WT", wt_fg, wt_bg);
        let wt_state_str = if state.oscillators_active { "On " } else { "Off" };
        fb.draw_text(22, 3, wt_state_str, wt_fg, wt_bg);
        let wt_vol_str = format!("{:3}", state.wt_volume);
        fb.draw_text(88, 3, &wt_vol_str, wt_fg, wt_bg);
        let wt_bar_w = (70i32 * state.wt_volume as i32 / 100).max(1);
        fb.fill_rect(48, 9, 70, 2, 0x0000);
        fb.fill_rect(48, 9, wt_bar_w, 2, if state.oscillators_active { 0xFFFF } else { 0x8410 });

        fb.fill_rect(116, 0, 8, 13, 0x0000);

        let gr_bg: u16 = if state.granular_active { 0x001F } else { 0x2945 };
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

        let key_octave = format!("{}{}", state.key_name(), state.octave);
        let scale_name = SCALE_NAMES[state.scale_index];
        let status = format!("{} {}", key_octave, scale_name);
        fb.draw_text(4, 16, &status, 0xFFFF, 0x0000);

        fb.fill_rect(0, 26, 240, 2, 0x4208);

        let lines = state.lines();
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

        self.write_full_framebuffer(&fb.to_bytes())?;
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

    pub fn draw_idle_screen(&mut self, state: &MenuState, hostname: &str) -> Result<()> {
        // Render the idle screen directly into the framebuffer.
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

        let x = ((240 - hostname.chars().count() as i32 * 8) / 2).max(0);
        fb.draw_text(x, 226, hostname, 0x4208, 0x0000);

        self.write_full_framebuffer(&fb.to_bytes())?;
        Ok(())
    }

    pub fn draw_powering_down_screen(&mut self) -> Result<()> {
        let mut fb = Framebuffer::new(240, 240);
        fb.clear(0x0000);

        let line1 = "Powering";
        let line1_w = line1.chars().count() as i32 * 16;
        let line1_x = (240 - line1_w) / 2;
        fb.draw_text_2x(line1_x, 96, line1, 0xF800, 0x0000);

        let line2 = "down";
        let line2_w = line2.chars().count() as i32 * 16;
        let line2_x = (240 - line2_w) / 2;
        fb.draw_text_2x(line2_x, 122, line2, 0xF800, 0x0000);

        self.write_full_framebuffer(&fb.to_bytes())?;
        Ok(())
    }

    pub fn clear_and_backlight_off(&mut self) -> Result<()> {
        let fb = Framebuffer::new(240, 240);
        self.write_full_framebuffer(&fb.to_bytes())?;
        if let Some(ref mut backlight) = self.backlight {
            backlight.set_low();
        }
        Ok(())
    }

    fn write_full_framebuffer(&mut self, bytes: &[u8]) -> Result<()> {
        self.command(0x2A, &[0x00, 0x00, 0x00, 0xEF])?;
        self.command(0x2B, &[0x00, 0x00, 0x00, 0xEF])?;
        self.dc.set_low();
        self.spi
            .write(&[0x2C])
            .map(|_| ())
            .context("failed writing ST7789 RAMWR command")?;
        self.dc.set_high();
        write_in_chunks(bytes, SPI_FRAMEBUFFER_CHUNK_SIZE, |chunk| {
            self.spi
                .write(chunk)
                .map(|_| ())
                .context("failed writing ST7789 framebuffer chunk")
        })
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

#[cfg(test)]
mod tests {
    use super::*;

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
