use std::fs;
use std::path::Path;
use anyhow::{Context, Result};
use crate::font::FONT_DATA;

pub(crate) struct Framebuffer {
    pub(crate) width: usize,
    pub(crate) height: usize,
    pixels: Vec<u16>,
}

impl Framebuffer {
    pub(crate) fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            pixels: vec![0; width * height],
        }
    }

    pub(crate) fn clear(&mut self, color: u16) {
        self.pixels.fill(color);
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
                        && (px as usize) < self.width
                        && (py as usize) < self.height
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
                                && (fpx as usize) < self.width
                                && (fpy as usize) < self.height
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
                                && (fpx as usize) < self.width
                                && (fpy as usize) < self.height
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
        self.pixels[y * self.width + x] = color;
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn get_pixel(&self, x: usize, y: usize) -> u16 {
        self.pixels[y * self.width + x]
    }

    pub(crate) fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(self.pixels.len() * 2);
        for px in &self.pixels {
            bytes.push((px >> 8) as u8);
            bytes.push((*px & 0xff) as u8);
        }
        bytes
    }

    pub(crate) fn save_ppm(&self, path: &Path) -> Result<()> {
        let mut out = Vec::with_capacity(self.pixels.len() * 3 + 32);
        out.extend_from_slice(format!("P6\n{} {}\n255\n", self.width, self.height).as_bytes());
        for px in &self.pixels {
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
