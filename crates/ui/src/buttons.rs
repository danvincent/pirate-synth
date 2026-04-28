use crate::menu::Button;
use anyhow::{Context, Result};
use rppal::gpio::{Gpio, InputPin};

pub struct ButtonConfig {
    pub pins: Vec<(u8, Button)>,
    pub shutdown_pin: Option<u8>,
}

impl ButtonConfig {
    pub fn new(pins: Vec<(u8, Button)>, shutdown_pin: Option<u8>) -> Result<Self> {
        let mut seen = std::collections::HashSet::new();
        for (pin, _) in &pins {
            if !seen.insert(*pin) {
                anyhow::bail!("duplicate BCM gpio pin mapping: {pin}");
            }
        }

        if let Some(pin) = shutdown_pin {
            if !seen.insert(pin) {
                anyhow::bail!("shutdown pin BCM gpio{pin} duplicates a button pin mapping");
            }
        }

        Ok(Self { pins, shutdown_pin })
    }

    /// Convenience constructor for the Pirate Audio HAT default wiring.
    pub fn pirate_audio() -> Self {
        Self {
            pins: vec![
                (5, Button::Up),
                (6, Button::Down),
                (16, Button::Select),
                (24, Button::Back),
            ],
            shutdown_pin: None,
        }
    }
}

pub struct ButtonReader {
    pins: Vec<InputPin>,
    mapping: Vec<Button>,
    last: Vec<bool>,
    shutdown_pin: Option<InputPin>,
}

impl ButtonReader {
    pub fn new(config: ButtonConfig) -> Result<Self> {
        let gpio = Gpio::new().context("failed to open GPIO controller")?;

        let mut pins = Vec::with_capacity(config.pins.len());
        let mut mapping = Vec::with_capacity(config.pins.len());
        for (bcm_pin, button) in config.pins {
            pins.push(open_input_pullup_pin(&gpio, bcm_pin)?);
            mapping.push(button);
        }

        let shutdown_pin = match config.shutdown_pin {
            Some(pin) => Some(open_input_pullup_pin(&gpio, pin)?),
            None => None,
        };

        Ok(Self {
            last: vec![false; pins.len()],
            pins,
            mapping,
            shutdown_pin,
        })
    }

    #[cfg(test)]
    fn from_config_for_test(config: ButtonConfig) -> Self {
        let mapping = config.pins.into_iter().map(|(_, button)| button).collect();
        Self {
            pins: Vec::new(),
            mapping,
            last: Vec::new(),
            shutdown_pin: None,
        }
    }

    pub fn poll_pressed(&mut self) -> Result<Option<Button>> {
        for (idx, pin) in self.pins.iter_mut().enumerate() {
            let pressed = pin.is_low();
            let rising = pressed && !self.last[idx];
            self.last[idx] = pressed;
            if rising {
                return Ok(Some(self.mapping[idx]));
            }
        }
        Ok(None)
    }

    /// Returns raw low-level state for every configured button pin.
    ///
    /// The index order matches the order of `ButtonConfig::pins`.
    /// `true` means the button is currently pressed (pin is low / active-low).
    pub fn raw_states(&self) -> Vec<bool> {
        self.pins.iter().map(InputPin::is_low).collect()
    }

    /// Fills `buf` with the current raw low-level state for every configured button pin,
    /// reusing the existing allocation to avoid per-call heap allocation.
    ///
    /// The index order matches `ButtonConfig::pins`.  `true` = pressed (active-low).
    pub fn raw_states_into(&self, buf: &mut Vec<bool>) {
        buf.clear();
        buf.extend(self.pins.iter().map(InputPin::is_low));
    }

    pub fn poll_shutdown_pin(&mut self) -> bool {
        match self.shutdown_pin.as_mut() {
            Some(pin) => pin.is_low(),
            None => false,
        }
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

fn open_input_pullup_pin(gpio: &Gpio, bcm_pin: u8) -> Result<InputPin> {
    let pin = gpio
        .get(bcm_pin)
        .with_context(|| format!("failed to open BCM gpio{bcm_pin}"))?
        .into_input_pullup();
    Ok(pin)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_button_config_two_pin_mapping() {
        let config = ButtonConfig::new(vec![(17, Button::Up), (27, Button::Down)], None)
            .expect("two unique pins should be accepted");

        assert_eq!(config.pins.len(), 2);
    }

    #[test]
    fn test_button_config_duplicate_pin_returns_error() {
        let result = ButtonConfig::new(vec![(17, Button::Up), (17, Button::Down)], None);

        assert!(result.is_err());
    }

    #[test]
    fn test_shutdown_pin_none_never_fires() {
        let config = ButtonConfig::new(vec![(17, Button::Up), (27, Button::Down)], None)
            .expect("valid button config");
        let mut reader = ButtonReader::from_config_for_test(config);

        assert!(!reader.poll_shutdown_pin());
    }

    #[test]
    fn test_button_config_duplicate_shutdown_pin_returns_error() {
        let result = ButtonConfig::new(vec![(17, Button::Up)], Some(17));
        assert!(result.is_err(), "shutdown pin duplicating a button pin must be rejected");
    }

    #[test]
    fn test_button_config_pirate_audio_has_four_pins() {
        let config = ButtonConfig::pirate_audio();
        assert_eq!(config.pins.len(), 4);
        assert!(config.shutdown_pin.is_none());
    }

    #[test]
    fn test_raw_states_into_fills_empty_reader() {
        let config = ButtonConfig::new(vec![], None).unwrap();
        let reader = ButtonReader::from_config_for_test(config);
        let mut buf = vec![true; 3]; // pre-fill to check it gets cleared
        reader.raw_states_into(&mut buf);
        assert!(buf.is_empty(), "raw_states_into must clear the buffer when there are no pins");
    }

    #[test]
    fn test_raw_states_into_reuses_buffer() {
        // Call raw_states_into twice to verify buf gets replaced each time.
        // The test reader has no hardware pins so both calls produce empty results.
        let config1 = ButtonConfig::new(vec![(5, Button::Up), (6, Button::Down)], None).unwrap();
        let reader1 = ButtonReader::from_config_for_test(config1);
        let mut buf = vec![true, false, true]; // pre-fill
        reader1.raw_states_into(&mut buf);
        // No hardware pins in the test reader, so buf is empty after the call
        assert!(buf.is_empty(), "raw_states_into must clear previous contents");

        // Second call with a different reader also clears and refills
        let config2 = ButtonConfig::new(vec![(16, Button::Select)], None).unwrap();
        let reader2 = ButtonReader::from_config_for_test(config2);
        reader2.raw_states_into(&mut buf);
        assert!(buf.is_empty(), "second raw_states_into must also produce empty (no hw pins)");
    }

    #[test]
    fn test_raw_states_returns_vec_with_correct_length() {
        // The test reader has no hardware pins so raw_states returns empty vec.
        let config = ButtonConfig::new(vec![(5, Button::Up), (6, Button::Down), (16, Button::Select)], None).unwrap();
        let reader = ButtonReader::from_config_for_test(config);
        // mapping has 3 entries but pins is empty (no hardware in test mode)
        let states = reader.raw_states();
        assert_eq!(states.len(), 0, "no hardware pins in test reader — raw_states must be empty");
    }

    #[test]
    fn test_sync_state_and_poll_pressed_after_sync() {
        // In test mode, sync_state iterates over empty pin/last lists.
        // Calling it then poll_pressed must both succeed without panicking.
        let config = ButtonConfig::new(vec![(5, Button::Up), (6, Button::Down)], None).unwrap();
        let mut reader = ButtonReader::from_config_for_test(config);
        reader.sync_state();
        // poll_pressed with no hardware pins must return Ok(None)
        assert_eq!(reader.poll_pressed().unwrap(), None);
    }

    #[test]
    fn test_poll_shutdown_pin_returns_false_when_none() {
        let config = ButtonConfig::new(vec![(5, Button::Up)], None).unwrap();
        let mut reader = ButtonReader::from_config_for_test(config);
        assert!(!reader.poll_shutdown_pin(), "poll_shutdown_pin must return false when shutdown_pin is None");
    }
}
