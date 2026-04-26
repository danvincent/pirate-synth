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

    /// Returns the raw low-level state of all four button pins as a `[bool; 4]`.
    ///
    /// Index mapping (matches the ordering used by [`Self::poll_pressed`]):
    /// - `[0]` → [`Button::Up`]
    /// - `[1]` → [`Button::Down`]
    /// - `[2]` → [`Button::Select`]
    /// - `[3]` → [`Button::Back`]
    ///
    /// `true` means the button is currently pressed (pin is low / active-low).
    pub fn raw_states(&self) -> [bool; 4] {
        [
            self.pins[0].is_low(),
            self.pins[1].is_low(),
            self.pins[2].is_low(),
            self.pins[3].is_low(),
        ]
    }

    /// Synchronises internal last-state tracking with current pin levels without
    /// returning an event. Call this after suppressing normal polling (e.g. at the
    /// end of a button combo) to prevent spurious rising edges on the next
    /// `poll_pressed` call.
    pub fn sync_state(&mut self) {
        for (last, pin) in self.last.iter_mut().zip(self.pins.iter()) {
            *last = pin.is_low();
        }
    }
}

fn open_input_pullup_pin(gpio: &Gpio, bcm_pin: u32) -> Result<InputPin> {
    let pin = gpio
        .get(bcm_pin as u8)
        .with_context(|| format!("failed to open BCM gpio{bcm_pin}"))?
        .into_input_pullup();
    Ok(pin)
}
