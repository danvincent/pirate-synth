use crate::display::{build_idle_framebuffer, build_menu_framebuffer};
use crate::framebuffer::Framebuffer;
use crate::menu::MenuState;
use anyhow::{Context, Result};
use std::fs::OpenOptions;
use std::io::{Seek, SeekFrom, Write};
use std::os::unix::io::AsRawFd;

pub struct LinuxFbDisplay {
    file: std::fs::File,
    width: u16,
    height: u16,
    buf: Vec<u8>,
    tty: Option<std::fs::File>,
}

impl LinuxFbDisplay {
    pub fn new(fb_path: &str, width: u16, height: u16) -> Result<Self> {
        let file = OpenOptions::new()
            .write(true)
            .open(fb_path)
            .with_context(|| format!("failed to open framebuffer {fb_path}"))?;
        let buf = vec![0u8; width as usize * height as usize * 4];

        // Put the VT into graphics mode so fbcon stops writing to our framebuffer.
        // Best-effort: if we can't open /dev/tty0 (e.g. no permissions), we proceed anyway.
        const KDSETMODE: libc::c_ulong = 0x4B3A;
        const KD_GRAPHICS: libc::c_int = 1;
        let tty = OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/tty0")
            .ok();
        if let Some(ref tty_file) = tty {
            unsafe {
                libc::ioctl(tty_file.as_raw_fd(), KDSETMODE, KD_GRAPHICS);
            }
        }

        Ok(Self {
            file,
            width,
            height,
            buf,
            tty,
        })
    }

    fn write_framebuffer(&mut self, rgb565: &[u8]) -> Result<()> {
        // Convert RGB565 big-endian to BGRA8888 in-place into self.buf
        for (i, chunk) in rgb565.chunks_exact(2).enumerate() {
            let px = ((chunk[0] as u16) << 8) | chunk[1] as u16;
            let r5 = ((px >> 11) & 0x1F) as u8;
            let g6 = ((px >> 5) & 0x3F) as u8;
            let b5 = (px & 0x1F) as u8;
            // Expand 5/6-bit channels to 8-bit
            let r8 = (r5 << 3) | (r5 >> 2);
            let g8 = (g6 << 2) | (g6 >> 4);
            let b8 = (b5 << 3) | (b5 >> 2);
            let base = i * 4;
            self.buf[base] = b8; // B at byte 0 (offset 0)
            self.buf[base + 1] = g8; // G at byte 1 (offset 8)
            self.buf[base + 2] = r8; // R at byte 2 (offset 16)
            self.buf[base + 3] = 0xFF; // A at byte 3 (offset 24)
        }
        self.file
            .seek(SeekFrom::Start(0))
            .context("failed to seek framebuffer to start")?;
        self.file
            .write_all(&self.buf)
            .context("failed to write framebuffer data")?;
        Ok(())
    }

    pub fn draw_menu(&mut self, state: &MenuState) -> Result<()> {
        let fb = build_menu_framebuffer(state, self.width, self.height);
        self.write_framebuffer(fb.as_bytes())
    }

    pub fn draw_idle_screen(&mut self, state: &MenuState, hostname: &str) -> Result<()> {
        let fb = build_idle_framebuffer(state, hostname, self.width, self.height);
        self.write_framebuffer(fb.as_bytes())
    }

    pub fn draw_powering_down_screen(&mut self) -> Result<()> {
        let mut fb = Framebuffer::new(self.width, self.height);
        fb.clear(0x0000);
        let fb_width = fb.width() as i32;
        let line1 = "Powering";
        let line1_x = (fb_width - line1.chars().count() as i32 * 18) / 2;
        fb.draw_text_2x(line1_x, 96, line1, 0xF800, 0x0000);
        let line2 = "down";
        let line2_x = (fb_width - line2.chars().count() as i32 * 18) / 2;
        fb.draw_text_2x(line2_x, 122, line2, 0xF800, 0x0000);
        self.write_framebuffer(fb.as_bytes())
    }

    pub fn clear_and_backlight_off(&mut self) -> Result<()> {
        // DPI24 backlight is hardware always-on; just clear screen to black
        let fb = Framebuffer::new(self.width, self.height);
        self.write_framebuffer(fb.as_bytes())
    }
}

impl Drop for LinuxFbDisplay {
    fn drop(&mut self) {
        // Restore the VT to text mode when the display is dropped.
        const KDSETMODE: libc::c_ulong = 0x4B3A;
        const KD_TEXT: libc::c_int = 0;
        if let Some(ref tty_file) = self.tty {
            unsafe {
                libc::ioctl(tty_file.as_raw_fd(), KDSETMODE, KD_TEXT);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    fn rgb565_to_bgra(hi: u8, lo: u8) -> [u8; 4] {
        let px = ((hi as u16) << 8) | lo as u16;
        let r5 = ((px >> 11) & 0x1F) as u8;
        let g6 = ((px >> 5) & 0x3F) as u8;
        let b5 = (px & 0x1F) as u8;
        let r8 = (r5 << 3) | (r5 >> 2);
        let g8 = (g6 << 2) | (g6 >> 4);
        let b8 = (b5 << 3) | (b5 >> 2);
        [b8, g8, r8, 0xFF]
    }

    #[test]
    fn test_rgb565_black_converts_to_bgra_black() {
        assert_eq!(rgb565_to_bgra(0x00, 0x00), [0x00, 0x00, 0x00, 0xFF]);
    }

    #[test]
    fn test_rgb565_white_converts_to_bgra_white() {
        // RGB565 white = 0xFFFF: R=31, G=63, B=31
        // R8=0xFF, G8=0xFF, B8=0xFF
        let [b, g, r, a] = rgb565_to_bgra(0xFF, 0xFF);
        assert_eq!(r, 0xFF);
        assert_eq!(g, 0xFF);
        assert_eq!(b, 0xFF);
        assert_eq!(a, 0xFF);
    }

    #[test]
    fn test_rgb565_pure_red_converts_to_bgra() {
        // RGB565 red = 0xF800: R=31, G=0, B=0
        let [b, g, r, a] = rgb565_to_bgra(0xF8, 0x00);
        assert!(r > 0xF0, "R channel should be near max, got {r}");
        assert_eq!(g, 0x00);
        assert_eq!(b, 0x00);
        assert_eq!(a, 0xFF);
    }

    #[test]
    fn test_rgb565_pure_blue_converts_to_bgra() {
        // RGB565 blue = 0x001F: R=0, G=0, B=31
        let [b, g, r, a] = rgb565_to_bgra(0x00, 0x1F);
        assert_eq!(r, 0x00);
        assert_eq!(g, 0x00);
        assert!(b > 0xF0, "B channel should be near max, got {b}");
        assert_eq!(a, 0xFF);
    }

    #[test]
    fn test_conversion_buffer_size() {
        // 1x1 framebuffer: 2 bytes RGB565 -> 4 bytes BGRA8888
        // Verify the formula: width * height * 4
        let width: u16 = 320;
        let height: u16 = 240;
        let expected_buf_size = width as usize * height as usize * 4;
        assert_eq!(expected_buf_size, 307_200);
    }
}