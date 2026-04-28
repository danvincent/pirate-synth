use anyhow::{Context, Result};
use rppal::gpio::OutputPin;
use rppal::spi::{Mode, Spi};

use crate::display::{
    build_idle_framebuffer, build_menu_framebuffer, SPI_CLOCK_HZ, SPI_FRAMEBUFFER_CHUNK_SIZE,
};
use crate::framebuffer::Framebuffer;
use crate::menu::MenuState;
use crate::parse_spi_device;

pub struct Ili9341Display {
    spi: Spi,
    dc: OutputPin,
    backlight: Option<OutputPin>,
    width: u16,
    height: u16,
}

impl Ili9341Display {
    pub fn new(spi_device: &str, dc_pin: u8, backlight_pin: Option<u8>) -> Result<Self> {
        let (bus, slave_select) = parse_spi_device(spi_device)?;
        let spi = Spi::new(bus, slave_select, SPI_CLOCK_HZ, Mode::Mode0)
            .with_context(|| format!("failed to open SPI device {spi_device}"))?;

        let gpio = rppal::gpio::Gpio::new().context("failed to open GPIO controller")?;
        let mut dc = gpio
            .get(dc_pin)
            .with_context(|| format!("failed to open BCM gpio{dc_pin} (display DC)"))?
            .into_output();
        dc.set_low();

        let backlight = match backlight_pin {
            Some(pin) => {
                let mut p = gpio
                    .get(pin)
                    .with_context(|| format!("failed to open BCM gpio{pin} (backlight)"))?
                    .into_output();
                p.set_low();
                Some(p)
            }
            None => None,
        };

        Ok(Self {
            spi,
            dc,
            backlight,
            width: 320,
            height: 240,
        })
    }

    pub fn caset_bytes() -> [u8; 4] {
        [0x00, 0x00, 0x01, 0x3F]
    }

    pub fn paset_bytes() -> [u8; 4] {
        [0x00, 0x00, 0x00, 0xEF]
    }

    pub fn init_commands() -> Vec<(u8, Vec<u8>)> {
        vec![
            (0x01, vec![]),
            (0xC0, vec![0x23]),
            (0xC1, vec![0x10]),
            (0xC5, vec![0x3E, 0x28]),
            (0xC7, vec![0x86]),
            (0x36, vec![0x68]),
            (0x3A, vec![0x55]),
            (0xB1, vec![0x00, 0x18]),
            (0xB6, vec![0x08, 0x82, 0x27]),
            (0x26, vec![0x01]),
            (
                0xE0,
                vec![
                    0x0F, 0x31, 0x2B, 0x0C, 0x0E, 0x08, 0x4E, 0xF1, 0x37, 0x07, 0x10, 0x03, 0x0E,
                    0x09, 0x00,
                ],
            ),
            (
                0xE1,
                vec![
                    0x00, 0x0E, 0x14, 0x03, 0x11, 0x07, 0x31, 0xC1, 0x48, 0x08, 0x0F, 0x0C, 0x31,
                    0x36, 0x0F,
                ],
            ),
            (0x11, vec![]),
            (0x29, vec![]),
        ]
    }

    pub fn init(&mut self) -> Result<()> {
        for (cmd, data) in Self::init_commands() {
            self.send_command(cmd)?;
            if !data.is_empty() {
                self.send_data(&data)?;
            }

            if cmd == 0x01 {
                std::thread::sleep(std::time::Duration::from_millis(150));
            } else if cmd == 0x11 {
                std::thread::sleep(std::time::Duration::from_millis(120));
            }
        }

        if let Some(backlight) = &mut self.backlight {
            backlight.set_high();
        }

        Ok(())
    }

    pub fn write_full_framebuffer(&mut self, fb: &Framebuffer) -> Result<()> {
        debug_assert_eq!(self.width, 320);
        debug_assert_eq!(self.height, 240);

        self.send_command(0x2A)?;
        self.send_data(&Self::caset_bytes())?;

        self.send_command(0x2B)?;
        self.send_data(&Self::paset_bytes())?;

        self.send_command(0x2C)?;
        for chunk in fb.as_bytes().chunks(SPI_FRAMEBUFFER_CHUNK_SIZE) {
            self.send_data(chunk)?;
        }

        Ok(())
    }

    pub fn draw_menu(&mut self, state: &MenuState) -> Result<()> {
        let fb = build_menu_framebuffer(state, self.width, self.height);
        self.write_full_framebuffer(&fb)
    }

    pub fn draw_idle_screen(&mut self, state: &MenuState, hostname: &str) -> Result<()> {
        let fb = build_idle_framebuffer(state, hostname, self.width, self.height);
        self.write_full_framebuffer(&fb)
    }

    pub fn draw_powering_down_screen(&mut self) -> Result<()> {
        let mut fb = Framebuffer::new(self.width, self.height);
        fb.clear(0x0000);
        let fb_width = fb.width() as i32;

        let text_width_2x = |text: &str| -> i32 {
            let chars = text.chars().count() as i32;
            if chars == 0 { 0 } else { chars * 18 - 2 }
        };

        let line1 = "Powering";
        let line1_w = text_width_2x(line1);
        let line1_x = (fb_width - line1_w) / 2;
        fb.draw_text_2x(line1_x, 96, line1, 0xF800, 0x0000);

        let line2 = "down";
        let line2_w = text_width_2x(line2);
        let line2_x = (fb_width - line2_w) / 2;
        fb.draw_text_2x(line2_x, 122, line2, 0xF800, 0x0000);

        self.write_full_framebuffer(&fb)
    }

    pub fn clear_and_backlight_off(&mut self) -> Result<()> {
        let fb = Framebuffer::new(self.width, self.height);
        self.write_full_framebuffer(&fb)?;
        if let Some(backlight) = &mut self.backlight {
            backlight.set_low();
        }
        Ok(())
    }

    fn send_command(&mut self, cmd: u8) -> Result<()> {
        self.dc.set_low();
        self.spi
            .write(&[cmd])
            .map(|_| ())
            .with_context(|| format!("failed writing ILI9341 command 0x{cmd:02X}"))
    }

    fn send_data(&mut self, data: &[u8]) -> Result<()> {
        self.dc.set_high();
        self.spi
            .write(data)
            .map(|_| ())
            .context("failed writing ILI9341 data")
    }
}

#[cfg(test)]
mod tests {
    use super::Ili9341Display;

    #[test]
    fn test_ili9341_window_caset_bytes() {
        assert_eq!(Ili9341Display::caset_bytes(), [0x00, 0x00, 0x01, 0x3F]);
    }

    #[test]
    fn test_ili9341_window_paset_bytes() {
        assert_eq!(Ili9341Display::paset_bytes(), [0x00, 0x00, 0x00, 0xEF]);
    }

    #[test]
    fn test_ili9341_init_sequence_commands() {
        let cmds: Vec<u8> = Ili9341Display::init_commands()
            .iter()
            .map(|(cmd, _)| *cmd)
            .collect();

        assert_eq!(
            cmds,
            vec![
                0x01, 0xC0, 0xC1, 0xC5, 0xC7, 0x36, 0x3A, 0xB1, 0xB6, 0x26, 0xE0, 0xE1, 0x11, 0x29,
            ]
        );
    }
}
