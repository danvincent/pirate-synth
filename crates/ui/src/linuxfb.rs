use crate::display::{build_idle_framebuffer, build_menu_framebuffer};
use crate::framebuffer::Framebuffer;
use crate::menu::MenuState;
use anyhow::{Context, Result};
use std::fs::OpenOptions;
use std::io::{Seek, SeekFrom, Write};
use std::os::unix::io::AsRawFd;

// Linux VT ioctl constants (from <linux/kd.h>).
// KDSETMODE switches the virtual terminal between text (KD_TEXT) and
// graphics mode (KD_GRAPHICS) so fbcon does not overwrite our framebuffer.
const KDSETMODE: libc::c_ulong = 0x4B3A;
const KD_GRAPHICS: libc::c_int = 1;
const KD_TEXT: libc::c_int = 0;

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
        let tty = OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/tty0")
            .ok();
        if let Some(ref tty_file) = tty {
            // SAFETY: tty_file fd is valid and owned; KDSETMODE is a standard Linux VT ioctl.
            // Failure is non-fatal (e.g. no /dev/tty0 access), so the result is discarded.
            let _ = unsafe { libc::ioctl(tty_file.as_raw_fd(), KDSETMODE, KD_GRAPHICS) };
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
        // text_width_2x computes the rendered pixel width of text drawn by draw_text_2x.
        // Each character advances 18px (16px glyph at 2× scale + 2px inter-glyph gap).
        // The last character has no trailing gap, so total = (n-1)*18 + 16 = n*18 - 2.
        let text_width_2x = |text: &str| -> i32 {
            let chars = text.chars().count() as i32;
            if chars == 0 { 0 } else { chars * 18 - 2 }
        };
        let line1 = "Powering";
        let line1_x = (fb_width - text_width_2x(line1)) / 2;
        fb.draw_text_2x(line1_x, 96, line1, 0xF800, 0x0000);
        let line2 = "down";
        let line2_x = (fb_width - text_width_2x(line2)) / 2;
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
        if let Some(ref tty_file) = self.tty {
            // SAFETY: tty_file fd is valid and owned; KDSETMODE is a standard Linux VT ioctl.
            // Failure during drop is non-fatal, so the result is discarded.
            let _ = unsafe { libc::ioctl(tty_file.as_raw_fd(), KDSETMODE, KD_TEXT) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::menu::MenuState;
    use std::env;
    use std::fs;

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

    /// RAII guard that deletes its file on drop, ensuring cleanup even if tests panic.
    struct TempFile(std::path::PathBuf);

    impl TempFile {
        /// Create an empty writable file and return a guard.
        fn new(tag: &str) -> Self {
            let path = env::temp_dir().join(format!("pirate_synth_test_fb_{tag}.bin"));
            fs::write(&path, b"").expect("failed to create temp fb file");
            TempFile(path)
        }

        fn path(&self) -> &std::path::Path {
            &self.0
        }
    }

    impl Drop for TempFile {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
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

    #[test]
    fn test_linuxfb_new_with_temp_file() {
        let tmp = TempFile::new("new");
        let display = LinuxFbDisplay::new(tmp.path().to_str().unwrap(), 240, 240);
        assert!(display.is_ok(), "LinuxFbDisplay::new must succeed with a valid writable file");
    }

    #[test]
    fn test_linuxfb_draw_menu_writes_correct_byte_count() {
        let tmp = TempFile::new("menu");
        let mut display = LinuxFbDisplay::new(tmp.path().to_str().unwrap(), 240, 240).unwrap();
        let state = MenuState::new(0.0, 4, 4);
        display.draw_menu(&state).expect("draw_menu must succeed");
        let written = fs::read(tmp.path()).unwrap();
        // 240 × 240 pixels × 4 bytes (BGRA8888) = 230 400
        assert_eq!(written.len(), 240 * 240 * 4, "draw_menu must write 240×240×4 bytes");
    }

    #[test]
    fn test_linuxfb_draw_menu_output_has_alpha_0xff() {
        let tmp = TempFile::new("menu_alpha");
        let mut display = LinuxFbDisplay::new(tmp.path().to_str().unwrap(), 240, 240).unwrap();
        let state = MenuState::new(0.0, 4, 4);
        display.draw_menu(&state).unwrap();
        let written = fs::read(tmp.path()).unwrap();
        // Every 4th byte is the alpha channel and must be 0xFF
        let all_opaque = written.chunks_exact(4).all(|px| px[3] == 0xFF);
        assert!(all_opaque, "alpha channel must be 0xFF for every pixel");
    }

    #[test]
    fn test_linuxfb_draw_idle_screen_writes_bytes() {
        let tmp = TempFile::new("idle");
        let mut display = LinuxFbDisplay::new(tmp.path().to_str().unwrap(), 240, 240).unwrap();
        let state = MenuState::new(0.0, 4, 4);
        display.draw_idle_screen(&state, "pirate").expect("draw_idle_screen must succeed");
        let written = fs::read(tmp.path()).unwrap();
        assert_eq!(written.len(), 240 * 240 * 4);
    }

    #[test]
    fn test_linuxfb_draw_idle_screen_differs_with_hostname() {
        let tmp_a = TempFile::new("idle_a");
        let tmp_b = TempFile::new("idle_b");
        let state = MenuState::new(0.0, 4, 4);

        let mut disp_a = LinuxFbDisplay::new(tmp_a.path().to_str().unwrap(), 240, 240).unwrap();
        disp_a.draw_idle_screen(&state, "hostname-a").unwrap();

        let mut disp_b = LinuxFbDisplay::new(tmp_b.path().to_str().unwrap(), 240, 240).unwrap();
        disp_b.draw_idle_screen(&state, "hostname-b").unwrap();

        let a = fs::read(tmp_a.path()).unwrap();
        let b = fs::read(tmp_b.path()).unwrap();
        assert_ne!(a, b, "idle screen output must differ for different hostnames");
    }

    #[test]
    fn test_linuxfb_draw_powering_down_screen_writes_bytes() {
        let tmp = TempFile::new("powerdown");
        let mut display = LinuxFbDisplay::new(tmp.path().to_str().unwrap(), 240, 240).unwrap();
        display
            .draw_powering_down_screen()
            .expect("draw_powering_down_screen must succeed");
        let written = fs::read(tmp.path()).unwrap();
        assert_eq!(written.len(), 240 * 240 * 4);
        // Red pixels (0xF800 → R=0xFF, G=0, B=0 → BGRA=[0,0,0xFF,0xFF]) must be present
        let has_red = written
            .chunks_exact(4)
            .any(|px| px[2] > 0xF0 && px[0] == 0 && px[1] == 0);
        assert!(has_red, "powering-down screen must contain at least one red pixel");
    }

    #[test]
    fn test_linuxfb_clear_and_backlight_off_writes_black_frame() {
        let tmp = TempFile::new("clear");
        let mut display = LinuxFbDisplay::new(tmp.path().to_str().unwrap(), 240, 240).unwrap();
        display
            .clear_and_backlight_off()
            .expect("clear_and_backlight_off must succeed");
        let written = fs::read(tmp.path()).unwrap();
        assert_eq!(written.len(), 240 * 240 * 4);
        // A cleared framebuffer is all black pixels: BGRA = [0, 0, 0, 0xFF]
        let all_black = written.chunks_exact(4).all(|px| px[0] == 0 && px[1] == 0 && px[2] == 0 && px[3] == 0xFF);
        assert!(all_black, "cleared framebuffer must contain only black pixels");
    }

    #[test]
    fn test_linuxfb_second_draw_overwrites_first() {
        let tmp = TempFile::new("overwrite");
        let mut display = LinuxFbDisplay::new(tmp.path().to_str().unwrap(), 240, 240).unwrap();
        let state = MenuState::new(0.0, 4, 4);
        display.draw_menu(&state).unwrap();
        display.clear_and_backlight_off().unwrap();
        // After clear, all pixels must be black
        let written = fs::read(tmp.path()).unwrap();
        let all_black = written.chunks_exact(4).all(|px| px[0] == 0 && px[1] == 0 && px[2] == 0 && px[3] == 0xFF);
        assert!(all_black, "second draw must overwrite first");
    }

    #[test]
    fn test_linuxfb_rgb565_green_conversion() {
        // 0x07E0 = pure green in RGB565 (G=63, R=0, B=0)
        let [b, g, r, a] = rgb565_to_bgra(0x07, 0xE0);
        assert_eq!(r, 0x00);
        assert!(g > 0xF0, "G channel must be near max for pure green");
        assert_eq!(b, 0x00);
        assert_eq!(a, 0xFF);
    }

    #[test]
    fn test_linuxfb_rgb565_various_colors() {
        // Cyan = 0x07FF: R=0, G=63, B=31
        let [b, g, r, _a] = rgb565_to_bgra(0x07, 0xFF);
        assert_eq!(r, 0x00);
        assert!(g > 0xF0);
        assert!(b > 0xF0);

        // Yellow = 0xFFE0: R=31, G=63, B=0
        let [b2, g2, r2, _a2] = rgb565_to_bgra(0xFF, 0xE0);
        assert!(r2 > 0xF0);
        assert!(g2 > 0xF0);
        assert_eq!(b2, 0x00);
    }
}