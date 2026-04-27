use std::fs;
use std::path::Path;
use anyhow::{Context, Result};
use crate::font::FONT_DATA;

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
            self.draw_char(x + (idx as i32 * 8), y, ch, fg, bg);
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
            self.draw_char_2x(x + (idx as i32 * 16), y, ch, fg, bg);
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
            self.draw_char_4x(x + (idx as i32 * 32), y, ch, fg, bg);
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

    pub(crate) fn to_bytes(&self) -> Vec<u8> {
        self.pixels.clone()
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
}
