use anyhow::{Context, Result};
use rppal::gpio::{Gpio, InputPin};
use crate::menu::Button;

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
