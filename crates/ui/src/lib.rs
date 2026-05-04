use anyhow::{Context, Result};
use rppal::spi::{Bus, SlaveSelect};

mod buttons;
mod display;
mod font;
mod framebuffer;
mod ili9341;
mod joystick;
mod linuxfb;
mod menu;

pub(crate) fn parse_spi_device(spi_path: &str) -> Result<(Bus, SlaveSelect)> {
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

pub use buttons::{ButtonConfig, ButtonReader};
pub use display::St7789Display;
pub use ili9341::Ili9341Display;
pub use joystick::JoystickButtonReader;
pub use linuxfb::LinuxFbDisplay;
pub use menu::{Button, MenuContext, MenuState, VideoStatus, BANK_NAMES, BYTEBEAT_ALGO_NAMES, KEY_NAMES, SCALE_NAMES};
