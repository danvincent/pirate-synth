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
        let fb = build_menu_framebuffer(state);
        self.write_full_framebuffer(&fb.to_bytes())?;
        Ok(())
    }

    pub fn draw_menu_to_ppm(state: &MenuState, path: &Path) -> Result<()> {
        build_menu_framebuffer(state).save_ppm(path)
    }

    pub fn draw_idle_screen(&mut self, state: &MenuState, hostname: &str) -> Result<()> {
        let fb = build_idle_framebuffer(state, hostname);
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

pub(crate) fn build_menu_framebuffer(state: &MenuState) -> Framebuffer {
    const VISIBLE_ROWS: usize = 11;
    let selected_item = state.selected_item;
    let scroll_offset = state.scroll_offset;

    let mut fb = Framebuffer::new(240, 240);
    fb.clear(0x0000);

    draw_menu_status_panel(&mut fb, "WT", state.oscillators_active, state.wt_volume, 0, 0x07E0, 0x2945);
    fb.fill_rect(116, 0, 8, 13, 0x0000);
    draw_menu_status_panel(&mut fb, "GR", state.granular_active, state.gr_volume, 124, 0x001F, 0x2945);

    let key_octave = format!("{}{}", state.key_name(), state.octave);
    let scale_name = SCALE_NAMES[state.scale_index];
    let status = format!("{} {}", key_octave, scale_name);
    fb.draw_text(4, 16, &status, 0xFFFF, 0x0000);

    fb.fill_rect(0, 26, 240, 2, 0x4208);

    let lines = state.lines();
    for (visual_row, line) in lines.iter().skip(scroll_offset).take(VISIBLE_ROWS).enumerate() {
        let abs_index = scroll_offset + visual_row;
        let y = 30 + (visual_row as i32 * 18);
        let selected = abs_index == selected_item;
        let bg = if selected { 0x07E0 } else { 0x0000 };
        let fg = if selected { 0x0000 } else { 0xFFFF };
        fb.fill_rect(2, y - 2, 236, 14, bg);
        fb.draw_text(4, y, line, fg, bg);
    }

    fb
}

/// Draw one WT or GR status panel in the menu header row.
/// `x_offset` is the left edge; `active_bg` and `inactive_bg` are the colour variants.
fn draw_menu_status_panel(
    fb: &mut Framebuffer,
    label: &str,
    active: bool,
    volume: u8,
    x_offset: i32,
    active_bg: u16,
    inactive_bg: u16,
) {
    let bg = if active { active_bg } else { inactive_bg };
    let fg: u16 = if active { 0x0000 } else { 0xFFFF };
    let state_str = if active { "On " } else { "Off" };
    let bar_color: u16 = if active { 0xFFFF } else { 0x8410 };

    fb.fill_rect(x_offset, 0, 116, 13, bg);
    fb.draw_text(x_offset + 3, 3, label, fg, bg);
    fb.draw_text(x_offset + 22, 3, state_str, fg, bg);
    let vol_str = format!("{:3}", volume);
    fb.draw_text(x_offset + 88, 3, &vol_str, fg, bg);
    let bar_w = (70i32 * volume as i32 / 100).max(1);
    fb.fill_rect(x_offset + 48, 9, 70, 2, 0x0000);
    fb.fill_rect(x_offset + 48, 9, bar_w, 2, bar_color);
}

pub(crate) fn build_idle_framebuffer(state: &MenuState, hostname: &str) -> Framebuffer {
    let mut fb = Framebuffer::new(240, 240);
    fb.clear(0x0000);

    let key = state.key_name();
    let key_total_w = key.chars().count() as i32 * 32;
    let key_x = (240 - key_total_w) / 2;
    fb.draw_text_4x(key_x, 10, key, 0xFFFF, 0x0000);

    let octave_str = format!("{}", state.octave);
    fb.draw_text_2x(key_x + key_total_w + 4, 28, &octave_str, 0x07E0, 0x0000);

    let scale = SCALE_NAMES[state.scale_index];
    let scale_w = scale.chars().count() as i32 * 16;
    fb.draw_text_2x((240 - scale_w) / 2, 58, scale, 0xAD55, 0x0000);

    draw_idle_volume_bar(&mut fb, state.oscillators_active, state.wt_volume, 35, 0x07E0, 0x2945, "WT");
    draw_idle_volume_bar(&mut fb, state.granular_active, state.gr_volume, 155, 0x001F, 0x2945, "GR");

    let wave_color: u16 = if state.oscillators_active || state.granular_active { 0x4208 } else { 0x2104 };
    draw_idle_sine_wave(&mut fb, 195, wave_color);

    let hostname_x = ((240 - hostname.chars().count() as i32 * 8) / 2).max(0);
    fb.draw_text(hostname_x, 226, hostname, 0x4208, 0x0000);

    fb
}

/// Draw one WT or GR volume bar with label in the idle screen.
fn draw_idle_volume_bar(
    fb: &mut Framebuffer,
    active: bool,
    volume: u8,
    x: i32,
    active_color: u16,
    inactive_color: u16,
    label: &str,
) {
    const BAR_MAX_H: i32 = 80;
    const BAR_W: i32 = 50;
    const BAR_TOP: i32 = 90;

    let color = if active { active_color } else { inactive_color };
    let bar_h = (BAR_MAX_H * volume as i32 / 100).max(2);
    fb.fill_rect(x, BAR_TOP, BAR_W, BAR_MAX_H, 0x1084);
    fb.fill_rect(x, BAR_TOP + (BAR_MAX_H - bar_h), BAR_W, bar_h, color);
    let label_color = if active { active_color } else { 0x8410 };
    fb.draw_text_2x(x + 9, BAR_TOP + BAR_MAX_H + 4, label, label_color, 0x0000);
    let vol_str = format!("{:3}", volume);
    fb.draw_text(x + 9, BAR_TOP + BAR_MAX_H + 22, &vol_str, 0xFFFF, 0x0000);
}

/// Draw a decorative sine-wave line across the idle screen.
fn draw_idle_sine_wave(fb: &mut Framebuffer, y_center: i32, color: u16) {
    for x in 0..240usize {
        let t = x as f32 * std::f32::consts::TAU / 240.0 * 2.5;
        let y_px = y_center + (t.sin() * 14.0) as i32;
        if y_px >= 0 && y_px < 239 {
            fb.set_pixel(x, y_px as usize, color);
            fb.set_pixel(x, (y_px + 1) as usize, color);
        }
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

    #[test]
    fn draw_menu_to_ppm_writes_valid_ppm_file() {
        use std::env;
        let state = MenuState::new(0.0, 4, 4);
        let path = env::temp_dir().join("pirate_synth_menu_test.ppm");
        St7789Display::draw_menu_to_ppm(&state, &path).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let header = b"P6\n240 240\n255\n";
        assert!(
            bytes.starts_with(header),
            "PPM file must start with P6 header for 240×240 image"
        );
        // P6 body: 240*240*3 bytes of pixel data
        assert_eq!(
            bytes.len(),
            header.len() + 240 * 240 * 3,
            "PPM file must contain correct pixel data size"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn build_menu_framebuffer_has_correct_dimensions() {
        let state = MenuState::new(0.0, 4, 4);
        let fb = build_menu_framebuffer(&state);
        assert_eq!(fb.width, 240);
        assert_eq!(fb.height, 240);
    }

    #[test]
    fn build_menu_framebuffer_encodes_oscillator_and_granular_state() {
        let mut state = MenuState::new(0.0, 4, 4);
        state.oscillators_active = true;
        state.granular_active = false;
        state.wt_volume = 80;
        state.gr_volume = 20;

        let fb_on = build_menu_framebuffer(&state);

        state.oscillators_active = false;
        let fb_off = build_menu_framebuffer(&state);

        // The WT header region should differ between active/inactive oscillators
        assert_ne!(
            fb_on.to_bytes(),
            fb_off.to_bytes(),
            "framebuffer must reflect oscillator active state"
        );
    }

    #[test]
    fn build_idle_framebuffer_has_correct_dimensions() {
        let state = MenuState::new(0.0, 4, 4);
        let fb = build_idle_framebuffer(&state, "myhost");
        assert_eq!(fb.width, 240);
        assert_eq!(fb.height, 240);
    }

    #[test]
    fn build_idle_framebuffer_encodes_granular_active_state() {
        let mut state = MenuState::new(0.0, 4, 4);
        state.oscillators_active = false;
        state.granular_active = true;

        let fb_active = build_idle_framebuffer(&state, "host");

        state.granular_active = false;
        let fb_inactive = build_idle_framebuffer(&state, "host");

        assert_ne!(
            fb_active.to_bytes(),
            fb_inactive.to_bytes(),
            "idle framebuffer must differ when granular_active changes"
        );
    }

    #[test]
    fn build_menu_framebuffer_highlights_correct_item_when_scrolled() {
        // With scroll_offset=3 and selected_item=5, item at visual row 2 should be highlighted.
        let mut state = MenuState::new(0.0, 8, 8);
        state.selected_item = 5;
        state.scroll_offset = 3;

        // Render with item 5 selected (scrolled so first visible is item 3)
        let fb_sel5 = build_menu_framebuffer(&state);

        // Now select a different item in the same visible window — framebuffer must differ
        state.selected_item = 4;
        let fb_sel4 = build_menu_framebuffer(&state);

        assert_ne!(
            fb_sel5.to_bytes(),
            fb_sel4.to_bytes(),
            "framebuffer must differ when a different visible row is selected"
        );
    }
}
