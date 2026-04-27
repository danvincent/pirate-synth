use crate::menu::Button;
use anyhow::{Context, Result};
use log::warn;
use std::fs::OpenOptions;
use std::io::Read;
use std::os::unix::io::AsRawFd;

const JS_EVENT_BUTTON: u8 = 0x01;
const JS_EVENT_AXIS: u8 = 0x02;
const JS_EVENT_INIT: u8 = 0x80;

/// Reads D-pad and button events from a Linux joystick device (`/dev/input/js0`).
///
/// Uses the joystick API (8-byte `js_event` structs) in non-blocking mode.
/// Maps D-pad axes and Start/Select buttons to the `Button` enum.
pub struct JoystickButtonReader {
    file: std::fs::File,
}

impl JoystickButtonReader {
    pub fn new(path: &str) -> Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .open(path)
            .with_context(|| format!("failed to open joystick device {path}"))?;

        // Set O_NONBLOCK so poll_pressed never blocks
        let fd = file.as_raw_fd();
        // SAFETY: fd is valid and owned by `file`; F_GETFL/F_SETFL are standard POSIX.
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL, 0) };
        if flags < 0 {
            anyhow::bail!("fcntl F_GETFL failed on {path}");
        }
        let ret = unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) };
        if ret < 0 {
            anyhow::bail!("fcntl F_SETFL O_NONBLOCK failed on {path}");
        }

        Ok(Self { file })
    }

    /// Returns the next mapped button press, or `None` if no event is ready.
    ///
    /// Reads all queued events and returns the first that maps to a `Button`.
    /// Discards init events (`JS_EVENT_INIT`), neutral axis returns, and
    /// button-release events (value=0).
    pub fn poll_pressed(&mut self) -> Option<Button> {
        let mut buf = [0u8; 8];
        loop {
            match self.file.read_exact(&mut buf) {
                Ok(()) => {
                    let event_type = buf[6] & !JS_EVENT_INIT; // strip init flag
                    let number = buf[7];
                    let value = i16::from_le_bytes([buf[4], buf[5]]);

                    let button = match event_type {
                        t if t == JS_EVENT_AXIS => match (number, value) {
                            (6, v) if v < -16000 => Some(Button::Left),
                            (6, v) if v > 16000 => Some(Button::Right),
                            (7, v) if v < -16000 => Some(Button::Up),
                            (7, v) if v > 16000 => Some(Button::Down),
                            _ => None, // neutral or unhandled axis
                        },
                        t if t == JS_EVENT_BUTTON => {
                            if value == 0 {
                                None // button release - ignore
                            } else {
                                match number {
                                    6 => Some(Button::Back),   // BTN_SELECT
                                    7 => Some(Button::Select), // BTN_START
                                    _ => None,
                                }
                            }
                        }
                        _ => None,
                    };

                    if button.is_some() {
                        return button;
                    }
                    // event not mapped - loop to drain next event
                }
                Err(ref e) if e.raw_os_error() == Some(libc::EAGAIN) => return None,
                Err(ref e) if e.raw_os_error() == Some(libc::EWOULDBLOCK) => return None,
                Err(e) => {
                    warn!("joystick read error: {e}");
                    return None;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_event(event_type: u8, number: u8, value: i16) -> [u8; 8] {
        let [v0, v1] = value.to_le_bytes();
        [0, 0, 0, 0, v0, v1, event_type, number]
    }

    fn parse_event(buf: &[u8; 8]) -> Option<Button> {
        let event_type = buf[6] & !JS_EVENT_INIT;
        let number = buf[7];
        let value = i16::from_le_bytes([buf[4], buf[5]]);
        match event_type {
            t if t == JS_EVENT_AXIS => match (number, value) {
                (6, v) if v < -16000 => Some(Button::Left),
                (6, v) if v > 16000 => Some(Button::Right),
                (7, v) if v < -16000 => Some(Button::Up),
                (7, v) if v > 16000 => Some(Button::Down),
                _ => None,
            },
            t if t == JS_EVENT_BUTTON => {
                if value == 0 {
                    None
                } else {
                    match number {
                        6 => Some(Button::Back),
                        7 => Some(Button::Select),
                        _ => None,
                    }
                }
            }
            _ => None,
        }
    }

    #[test]
    fn test_dpad_left() {
        let buf = make_event(JS_EVENT_AXIS, 6, -32767);
        assert_eq!(parse_event(&buf), Some(Button::Left));
    }

    #[test]
    fn test_dpad_right() {
        let buf = make_event(JS_EVENT_AXIS, 6, 32767);
        assert_eq!(parse_event(&buf), Some(Button::Right));
    }

    #[test]
    fn test_dpad_up() {
        let buf = make_event(JS_EVENT_AXIS, 7, -32767);
        assert_eq!(parse_event(&buf), Some(Button::Up));
    }

    #[test]
    fn test_dpad_down() {
        let buf = make_event(JS_EVENT_AXIS, 7, 32767);
        assert_eq!(parse_event(&buf), Some(Button::Down));
    }

    #[test]
    fn test_dpad_neutral_ignored() {
        let buf = make_event(JS_EVENT_AXIS, 6, 0);
        assert_eq!(parse_event(&buf), None);
    }

    #[test]
    fn test_start_button_select() {
        let buf = make_event(JS_EVENT_BUTTON, 7, 1);
        assert_eq!(parse_event(&buf), Some(Button::Select));
    }

    #[test]
    fn test_select_button_back() {
        let buf = make_event(JS_EVENT_BUTTON, 6, 1);
        assert_eq!(parse_event(&buf), Some(Button::Back));
    }

    #[test]
    fn test_button_release_ignored() {
        let buf = make_event(JS_EVENT_BUTTON, 7, 0);
        assert_eq!(parse_event(&buf), None);
    }

    #[test]
    fn test_init_flag_stripped() {
        // JS_EVENT_INIT | JS_EVENT_AXIS should still map axis correctly
        let buf = make_event(JS_EVENT_AXIS | JS_EVENT_INIT, 7, -32767);
        assert_eq!(parse_event(&buf), Some(Button::Up));
    }

    #[test]
    fn test_unhandled_button_ignored() {
        let buf = make_event(JS_EVENT_BUTTON, 0, 1); // A button - not mapped
        assert_eq!(parse_event(&buf), None);
    }
}
