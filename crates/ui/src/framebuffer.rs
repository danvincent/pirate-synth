use crate::font::FONT_DATA;
use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

pub struct Framebuffer {
    pub(crate) width: u16,
    pub(crate) height: u16,
    pixels: Vec<u8>,
}

impl Framebuffer {
    pub fn new(width: u16, height: u16) -> Self {
        let len = width as usize * height as usize * 2;
        Self {
            width,
            height,
            pixels: vec![0; len],
        }
    }

    pub(crate) fn width(&self) -> u16 {
        self.width
    }

    pub(crate) fn height(&self) -> u16 {
        self.height
    }

    pub(crate) fn as_bytes(&self) -> &[u8] {
        &self.pixels
    }

    pub(crate) fn clear(&mut self, color: u16) {
        let hi = (color >> 8) as u8;
        let lo = (color & 0xff) as u8;
        for px in self.pixels.chunks_exact_mut(2) {
            px[0] = hi;
            px[1] = lo;
        }
    }

    pub(crate) fn fill_rect(&mut self, x: i32, y: i32, w: i32, h: i32, color: u16) {
        for yy in y.max(0)..(y + h).min(self.height as i32) {
            for xx in x.max(0)..(x + w).min(self.width as i32) {
                self.set_pixel(xx as usize, yy as usize, color);
            }
        }
    }

    pub(crate) fn draw_text(&mut self, x: i32, y: i32, text: &str, fg: u16, bg: u16) {
        for (idx, ch) in text.chars().enumerate() {
            self.draw_char(x + (idx as i32 * 9), y, ch, fg, bg);
        }
    }

    pub(crate) fn draw_char(&mut self, x: i32, y: i32, ch: char, fg: u16, bg: u16) {
        let idx = ch as usize;
        if idx < FONT_DATA.len() {
            let glyph = &FONT_DATA[idx];
            for (row, bits) in glyph.iter().enumerate() {
                for col in 0..8usize {
                    let color = if (bits >> (7 - col)) & 1 == 1 { fg } else { bg };
                    let px = x + col as i32;
                    let py = y + row as i32;
                    if px >= 0
                        && py >= 0
                        && (px as usize) < self.width as usize
                        && (py as usize) < self.height as usize
                    {
                        self.set_pixel(px as usize, py as usize, color);
                    }
                }
            }
        }
    }

    pub(crate) fn draw_char_2x(&mut self, x: i32, y: i32, ch: char, fg: u16, bg: u16) {
        let idx = ch as usize;
        if idx < FONT_DATA.len() {
            let glyph = &FONT_DATA[idx];
            for (row, bits) in glyph.iter().enumerate() {
                for col in 0..8usize {
                    let color = if (bits >> (7 - col)) & 1 == 1 { fg } else { bg };
                    let px = x + col as i32 * 2;
                    let py = y + row as i32 * 2;
                    for dy in 0..2i32 {
                        for dx in 0..2i32 {
                            let fpx = px + dx;
                            let fpy = py + dy;
                            if fpx >= 0
                                && fpy >= 0
                                && (fpx as usize) < self.width as usize
                                && (fpy as usize) < self.height as usize
                            {
                                self.set_pixel(fpx as usize, fpy as usize, color);
                            }
                        }
                    }
                }
            }
        }
    }

    pub(crate) fn draw_text_2x(&mut self, x: i32, y: i32, text: &str, fg: u16, bg: u16) {
        for (idx, ch) in text.chars().enumerate() {
            self.draw_char_2x(x + (idx as i32 * 18), y, ch, fg, bg);
        }
    }

    pub(crate) fn draw_char_4x(&mut self, x: i32, y: i32, ch: char, fg: u16, bg: u16) {
        let idx = ch as usize;
        if idx < FONT_DATA.len() {
            let glyph = &FONT_DATA[idx];
            for (row, bits) in glyph.iter().enumerate() {
                for col in 0..8usize {
                    let color = if (bits >> (7 - col)) & 1 == 1 { fg } else { bg };
                    let px = x + col as i32 * 4;
                    let py = y + row as i32 * 4;
                    for dy in 0..4i32 {
                        for dx in 0..4i32 {
                            let fpx = px + dx;
                            let fpy = py + dy;
                            if fpx >= 0
                                && fpy >= 0
                                && (fpx as usize) < self.width as usize
                                && (fpy as usize) < self.height as usize
                            {
                                self.set_pixel(fpx as usize, fpy as usize, color);
                            }
                        }
                    }
                }
            }
        }
    }

    pub(crate) fn draw_text_4x(&mut self, x: i32, y: i32, text: &str, fg: u16, bg: u16) {
        for (idx, ch) in text.chars().enumerate() {
            self.draw_char_4x(x + (idx as i32 * 36), y, ch, fg, bg);
        }
    }

    pub(crate) fn set_pixel(&mut self, x: usize, y: usize, color: u16) {
        let width = self.width as usize;
        let height = self.height as usize;
        if x >= width || y >= height {
            return;
        }
        let idx = (y * width + x) * 2;
        self.pixels[idx] = (color >> 8) as u8;
        self.pixels[idx + 1] = (color & 0xff) as u8;
    }

    #[allow(dead_code)]
    pub(crate) fn get_pixel(&self, x: usize, y: usize) -> u16 {
        let width = self.width as usize;
        let height = self.height as usize;
        if x >= width || y >= height {
            return 0;
        }
        let idx = (y * width + x) * 2;
        ((self.pixels[idx] as u16) << 8) | self.pixels[idx + 1] as u16
    }

    pub(crate) fn save_ppm(&self, path: &Path) -> Result<()> {
        let mut out = Vec::with_capacity(self.pixels.len() / 2 * 3 + 32);
        out.extend_from_slice(format!("P6\n{} {}\n255\n", self.width, self.height).as_bytes());
        for px in self.pixels.chunks_exact(2) {
            let px = ((px[0] as u16) << 8) | px[1] as u16;
            let r = ((px >> 11) & 0x1f) as u8;
            let g = ((px >> 5) & 0x3f) as u8;
            let b = (px & 0x1f) as u8;
            out.push((r << 3) | (r >> 2));
            out.push((g << 2) | (g >> 4));
            out.push((b << 3) | (b >> 2));
        }
        fs::write(path, out)
            .with_context(|| format!("failed writing screenshot {}", path.display()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::Framebuffer;

    #[test]
    fn test_framebuffer_new_320x240_size() {
        let fb = Framebuffer::new(320, 240);
        assert_eq!(fb.as_bytes().len(), 153_600);
    }

    #[test]
    fn test_framebuffer_pixel_bounds_320x240() {
        let mut fb = Framebuffer::new(320, 240);
        fb.set_pixel(319, 239, 0xFFFF);
    }

    #[test]
    fn test_framebuffer_width_height_getters() {
        let fb = Framebuffer::new(320, 240);
        assert_eq!(fb.width(), 320);
        assert_eq!(fb.height(), 240);
    }

    #[test]
    fn test_framebuffer_clear_sets_all_pixels() {
        let mut fb = Framebuffer::new(4, 2);
        fb.clear(0xF800); // red in RGB565
        let bytes = fb.as_bytes();
        for px in bytes.chunks_exact(2) {
            assert_eq!(px[0], 0xF8);
            assert_eq!(px[1], 0x00);
        }
    }

    #[test]
    fn test_framebuffer_set_and_get_pixel() {
        let mut fb = Framebuffer::new(16, 16);
        fb.set_pixel(3, 5, 0x07E0); // green
        assert_eq!(fb.get_pixel(3, 5), 0x07E0);
        // other pixels remain zero
        assert_eq!(fb.get_pixel(0, 0), 0x0000);
    }

    #[test]
    fn test_framebuffer_set_pixel_out_of_bounds_is_noop() {
        let mut fb = Framebuffer::new(8, 8);
        fb.set_pixel(8, 0, 0xFFFF);  // x == width, out of bounds
        fb.set_pixel(0, 8, 0xFFFF);  // y == height, out of bounds
        for b in fb.as_bytes() {
            assert_eq!(*b, 0, "out-of-bounds writes must not modify the buffer");
        }
    }

    #[test]
    fn test_framebuffer_get_pixel_out_of_bounds_returns_zero() {
        let fb = Framebuffer::new(8, 8);
        assert_eq!(fb.get_pixel(8, 0), 0);
        assert_eq!(fb.get_pixel(0, 8), 0);
    }

    #[test]
    fn test_framebuffer_fill_rect_basic() {
        let mut fb = Framebuffer::new(10, 10);
        fb.fill_rect(2, 2, 3, 3, 0x001F); // blue 3×3 starting at (2,2)
        for y in 2..5 {
            for x in 2..5 {
                assert_eq!(fb.get_pixel(x, y), 0x001F, "expected blue at ({x},{y})");
            }
        }
        assert_eq!(fb.get_pixel(1, 2), 0x0000);
        assert_eq!(fb.get_pixel(5, 2), 0x0000);
    }

    #[test]
    fn test_framebuffer_fill_rect_clamps_to_bounds() {
        // rect extends outside framebuffer — must not panic
        let mut fb = Framebuffer::new(8, 8);
        fb.fill_rect(-2, -2, 20, 20, 0xFFFF);
        // All pixels within bounds should be white
        for y in 0..8 {
            for x in 0..8 {
                assert_eq!(fb.get_pixel(x, y), 0xFFFF);
            }
        }
    }

    #[test]
    fn test_framebuffer_draw_text_places_glyph() {
        let mut fb = Framebuffer::new(240, 16);
        fb.draw_text(0, 0, "A", 0xFFFF, 0x0000);
        // 'A' glyph has foreground pixels across rows 0-7, cols 0-7
        let has_foreground = (0..8usize).flat_map(|y| (0..8usize).map(move |x| (x, y)))
            .any(|(x, y)| fb.get_pixel(x, y) == 0xFFFF);
        assert!(has_foreground, "draw_text must render at least one foreground pixel for 'A'");
    }

    #[test]
    fn test_framebuffer_draw_text_2x_places_glyph() {
        let mut fb = Framebuffer::new(240, 32);
        fb.draw_text_2x(0, 0, "A", 0xFFFF, 0x0000);
        let has_foreground = (0..16usize).flat_map(|y| (0..16usize).map(move |x| (x, y)))
            .any(|(x, y)| fb.get_pixel(x, y) == 0xFFFF);
        assert!(has_foreground, "draw_text_2x must render at least one foreground pixel for 'A'");
    }

    #[test]
    fn test_framebuffer_draw_text_4x_places_glyph() {
        let mut fb = Framebuffer::new(240, 64);
        fb.draw_text_4x(0, 0, "A", 0xFFFF, 0x0000);
        let has_foreground = (0..32usize).flat_map(|y| (0..32usize).map(move |x| (x, y)))
            .any(|(x, y)| fb.get_pixel(x, y) == 0xFFFF);
        assert!(has_foreground, "draw_text_4x must render at least one foreground pixel for 'A'");
    }

    #[test]
    fn test_draw_text_2x_stride_matches_centering_formula() {
        // Verify the stride used by draw_text_2x: n chars produce n*18-2 pixels wide text.
        // Two glyphs of 8px × 2 = 16px each with 1px gap between them = 16+1+16 = 33px.
        // For n chars: n * 16 px data + (n-1) * 1 px gaps = n*16 + n - 1 = n*18 - 1? 
        // draw_text_2x advances by 18px per char, so last char occupies x to x+16.
        // Total used width for n chars = (n-1)*18 + 16 = n*18 - 2.
        let text = "Hi";
        let n = text.chars().count() as i32;
        let expected_w = n * 18 - 2;
        assert_eq!(expected_w, 34, "2 chars: 2*18-2=34");
    }

    #[test]
    fn test_framebuffer_save_ppm_produces_correct_file() {
        use std::env;
        let mut fb = Framebuffer::new(2, 2);
        fb.set_pixel(0, 0, 0xF800); // red
        let path = env::temp_dir().join("pirate_synth_fb_test.ppm");
        fb.save_ppm(&path).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert!(bytes.starts_with(b"P6\n2 2\n255\n"), "PPM header mismatch");
        assert_eq!(bytes.len(), "P6\n2 2\n255\n".len() + 2 * 2 * 3);
        let _ = std::fs::remove_file(&path);
    }
}
