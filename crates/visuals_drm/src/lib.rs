use std::fs::{read_dir, read_to_string, File, OpenOptions};
use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::fs::FileExt;
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use crossbeam_channel::{bounded, Receiver, Sender};
use log::{debug, info};

const FBIOGET_VSCREENINFO: libc::c_ulong = 0x4600;
const FBIOGET_FSCREENINFO: libc::c_ulong = 0x4602;
const TARGET_WIDTH: usize = 640;
const TARGET_HEIGHT: usize = 480;

#[repr(C)]
#[derive(Clone, Copy)]
struct FbBitfield {
    offset: u32,
    length: u32,
    msb_right: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct FbVarScreeninfo {
    xres: u32,
    yres: u32,
    xres_virtual: u32,
    yres_virtual: u32,
    xoffset: u32,
    yoffset: u32,
    bits_per_pixel: u32,
    grayscale: u32,
    red: FbBitfield,
    green: FbBitfield,
    blue: FbBitfield,
    transp: FbBitfield,
    nonstd: u32,
    activate: u32,
    height: u32,
    width: u32,
    accel_flags: u32,
    pixclock: u32,
    left_margin: u32,
    right_margin: u32,
    upper_margin: u32,
    lower_margin: u32,
    hsync_len: u32,
    vsync_len: u32,
    sync: u32,
    vmode: u32,
    rotate: u32,
    colorspace: u32,
    reserved: [u32; 4],
}

impl Default for FbVarScreeninfo {
    fn default() -> Self {
        // SAFETY: C struct allows zero initialization.
        unsafe { std::mem::zeroed() }
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
struct FbFixScreeninfo {
    id: [libc::c_char; 16],
    smem_start: libc::c_ulong,
    smem_len: u32,
    type_: u32,
    type_aux: u32,
    visual: u32,
    xpanstep: u16,
    ypanstep: u16,
    ywrapstep: u16,
    line_length: u32,
    mmio_start: libc::c_ulong,
    mmio_len: u32,
    accel: u32,
    capabilities: u16,
    reserved: [u16; 2],
}

impl Default for FbFixScreeninfo {
    fn default() -> Self {
        // SAFETY: C struct allows zero initialization.
        unsafe { std::mem::zeroed() }
    }
}

#[derive(Clone, Copy)]
enum PixelFormat {
    Rgb565,
    Xrgb8888,
}

struct Framebuffer {
    file: File,
    width: usize,
    height: usize,
    line_length: usize,
    format: PixelFormat,
}

#[derive(Debug)]
pub enum VisualsInitError {
    NoHdmi,
    Init(anyhow::Error),
}

impl std::fmt::Display for VisualsInitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoHdmi => write!(f, "no connected HDMI connector found"),
            Self::Init(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for VisualsInitError {}

pub fn try_spawn_visuals() -> std::result::Result<Sender<f32>, VisualsInitError> {
    if !has_connected_hdmi().map_err(|err| VisualsInitError::Init(err.into()))? {
        return Err(VisualsInitError::NoHdmi);
    }

    OpenOptions::new()
        .read(true)
        .open("/dev/dri/card0")
        .context("failed to open /dev/dri/card0 for DRM probe")
        .map_err(VisualsInitError::Init)?;

    let fb = open_framebuffer().map_err(VisualsInitError::Init)?;
    let (tx, rx) = bounded::<f32>(1);
    thread::spawn(move || {
        if let Err(err) = run_render_loop(fb, rx) {
            log::warn!("HDMI visuals renderer exited: {err:#}");
        }
    });
    Ok(tx)
}

fn has_connected_hdmi() -> io::Result<bool> {
    let base = Path::new("/sys/class/drm");
    if !base.exists() {
        return Ok(false);
    }
    for entry in read_dir(base)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.contains("HDMI") {
            continue;
        }
        let status_path = entry.path().join("status");
        match read_to_string(&status_path) {
            Ok(status) if status.trim() == "connected" => return Ok(true),
            Ok(_) => continue,
            Err(err) => {
                debug!(
                    "failed reading DRM connector status from {}: {}",
                    status_path.display(),
                    err
                );
            }
        }
    }
    Ok(false)
}

fn open_framebuffer() -> Result<Framebuffer> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/fb0")
        .context("failed to open /dev/fb0")?;
    let mut var = FbVarScreeninfo::default();
    let mut fix = FbFixScreeninfo::default();
    // SAFETY: ioctl expects valid pointers to writable C structs.
    let get_var_result = unsafe { libc::ioctl(file.as_raw_fd(), FBIOGET_VSCREENINFO, &mut var) };
    if get_var_result < 0 {
        return Err(io::Error::last_os_error()).context("FBIOGET_VSCREENINFO failed");
    }
    // SAFETY: ioctl expects valid pointers to writable C structs.
    let get_fix_result = unsafe { libc::ioctl(file.as_raw_fd(), FBIOGET_FSCREENINFO, &mut fix) };
    if get_fix_result < 0 {
        return Err(io::Error::last_os_error()).context("FBIOGET_FSCREENINFO failed");
    }
    let format = match var.bits_per_pixel {
        16 => PixelFormat::Rgb565,
        32 => PixelFormat::Xrgb8888,
        other => anyhow::bail!("unsupported fb0 format {}bpp", other),
    };
    info!(
        "HDMI visuals framebuffer ready: {}x{} {}bpp",
        var.xres, var.yres, var.bits_per_pixel
    );
    Ok(Framebuffer {
        file,
        width: var.xres as usize,
        height: var.yres as usize,
        line_length: fix.line_length as usize,
        format,
    })
}

fn run_render_loop(fb: Framebuffer, level_rx: Receiver<f32>) -> Result<()> {
    let mut frame = vec![0u8; fb.line_length * fb.height];
    let mut frame_index: u32 = 0;
    let mut smoothed_level = 0.0f32;
    let mut target_level = 0.0f32;
    let mut last_non_silent = Instant::now();
    loop {
        while let Ok(level) = level_rx.try_recv() {
            target_level = level.clamp(0.0, 1.0);
            if target_level > 0.01 {
                last_non_silent = Instant::now();
            }
        }
        smoothed_level = smoothed_level * 0.86 + target_level * 0.14;
        let silence_age = Instant::now().duration_since(last_non_silent);
        let quiet_mode = silence_age > Duration::from_secs(2);
        let intensity = if quiet_mode {
            smoothed_level * 0.35
        } else {
            smoothed_level
        };

        render_static_layers(&mut frame, &fb, intensity, frame_index);
        fb.file
            .write_all_at(&frame, 0)
            .context("failed writing framebuffer")?;

        frame_index = frame_index.wrapping_add(1);
        let frame_delay = if quiet_mode {
            Duration::from_millis(125)
        } else {
            Duration::from_millis(33)
        };
        thread::sleep(frame_delay);
    }
}

fn render_static_layers(frame: &mut [u8], fb: &Framebuffer, level: f32, frame_index: u32) {
    frame.fill(0);
    let render_width = fb.width.min(TARGET_WIDTH);
    let render_height = fb.height.min(TARGET_HEIGHT);
    let x_offset = (fb.width.saturating_sub(render_width)) / 2;
    let y_offset = (fb.height.saturating_sub(render_height)) / 2;

    let block_base = (48.0 - (44.0 * level.powf(0.7))).round() as usize;
    let block_base = block_base.clamp(2, 64);
    let layer_scales = [4usize, 2, 1, 1];
    let layer_divisors = [1usize, 1, 2, 4];
    let layer_alpha = [0.25f32, 0.45, 0.7, 1.0];

    for layer in 0..layer_scales.len() {
        let block = (block_base * layer_scales[layer] / layer_divisors[layer]).clamp(1, 96);
        let x_shift = (frame_index as usize / (2 + layer)).wrapping_mul(3 + layer) % block;
        let y_shift = (frame_index as usize / (3 + layer)).wrapping_mul(2 + layer) % block;

        for y in (0..render_height).step_by(block) {
            for x in (0..render_width).step_by(block) {
                let seed = ((x + x_shift) as u32)
                    ^ (((y + y_shift) as u32) << 10)
                    ^ (frame_index << (layer + 1))
                    ^ ((layer as u32 + 1) * 0x9E37);
                let noise = xorshift32(seed);
                let mut gray = (noise & 0xff) as f32;
                gray *= layer_alpha[layer];
                gray *= 0.25 + level * 0.9;
                gray = gray.clamp(0.0, 255.0);
                let gray = gray as u8;
                fill_block(
                    frame,
                    fb,
                    x + x_offset,
                    y + y_offset,
                    block,
                    block,
                    gray,
                );
            }
        }
    }
}

fn xorshift32(mut state: u32) -> u32 {
    if state == 0 {
        state = 0xA341_316C;
    }
    state ^= state << 13;
    state ^= state >> 17;
    state ^= state << 5;
    state
}

fn fill_block(
    frame: &mut [u8],
    fb: &Framebuffer,
    x: usize,
    y: usize,
    width: usize,
    height: usize,
    gray: u8,
) {
    let end_x = (x + width).min(fb.width);
    let end_y = (y + height).min(fb.height);
    for yy in y..end_y {
        let row_start = yy * fb.line_length;
        for xx in x..end_x {
            let pixel_offset = match fb.format {
                PixelFormat::Rgb565 => row_start + xx * 2,
                PixelFormat::Xrgb8888 => row_start + xx * 4,
            };
            match fb.format {
                PixelFormat::Rgb565 => {
                    let r = (gray as u16 >> 3) & 0x1f;
                    let g = (gray as u16 >> 2) & 0x3f;
                    let b = (gray as u16 >> 3) & 0x1f;
                    let packed = (r << 11) | (g << 5) | b;
                    let bytes = packed.to_le_bytes();
                    frame[pixel_offset] = bytes[0];
                    frame[pixel_offset + 1] = bytes[1];
                }
                PixelFormat::Xrgb8888 => {
                    frame[pixel_offset] = gray;
                    frame[pixel_offset + 1] = gray;
                    frame[pixel_offset + 2] = gray;
                    frame[pixel_offset + 3] = 0;
                }
            }
        }
    }
}
