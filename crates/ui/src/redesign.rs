use anyhow::Result;
use std::path::Path;
use crate::framebuffer::Framebuffer;
use crate::menu::{MenuState, VideoStatus, SCALE_NAMES, BANK_NAMES};

/// Render two mockup frames showing the proposed UI redesign to PPM files in `dir`.
/// Screen 1: top of list, item 0 selected.
/// Screen 2: scrolled down, item 10 (WT OSCS) selected.
pub fn draw_redesign_mockups_ppm(dir: &Path) -> Result<()> {
    let state = MenuState {
        key_index: 9,          // A
        octave: 1,
        fine_tune_cents: 0.0,
        stereo_spread: 100,
        scale_index: 7,        // HIRAJOSHI
        bank_index: 0,
        wt_volume: 50,
        gr_volume: 50,
        oscillators_active: true,
        granular_active: false,
        osc_count: 8,
        gr_voices: 8,
        video_status: VideoStatus::Off,
        glide_progress: None,
        selected_item: 0,
        scroll_offset: 0,
    };

    // Screen 1: cursor at item 0, no scroll
    render_redesign_frame_to_ppm(
        &state,
        0,
        0,
        &dir.join("redesign-screen1-top.ppm"),
    )?;

    // Screen 2: cursor at item 10 (WT OSCS), scrolled (scroll_offset=1)
    render_redesign_frame_to_ppm(
        &state,
        10,
        1,
        &dir.join("redesign-screen2-scrolled.ppm"),
    )?;

    // Screen 3: idle overview screen
    render_idle_screen_to_ppm(
        &state,
        &dir.join("redesign-screen3-idle.ppm"),
    )?;

    Ok(())
}

fn render_redesign_frame_to_ppm(
    state: &MenuState,
    selected_item: usize,
    scroll_offset: usize,
    path: &Path,
) -> Result<()> {
    const VISIBLE_ROWS: usize = 11;

    let mut fb = Framebuffer::new(240, 240);
    fb.clear(0x0000);

    // ── Graphical header (no title, 26px) ─────────────────────────────────────
    // Left block: WT status (green if ON, dark if OFF)
    let wt_bg: u16 = if state.oscillators_active { 0x07E0 } else { 0x2945 };
    let wt_fg: u16 = if state.oscillators_active { 0x0000 } else { 0xFFFF };
    fb.fill_rect(0, 0, 116, 13, wt_bg);
    fb.draw_text(3, 3, "WT", wt_fg, wt_bg);
    let wt_state_str = if state.oscillators_active { "On " } else { "Off" };
    fb.draw_text(22, 3, wt_state_str, wt_fg, wt_bg);
    let wt_vol_str = format!("{:3}", state.wt_volume);
    fb.draw_text(88, 3, &wt_vol_str, wt_fg, wt_bg);
    // Volume bar (inside block, y=9, 2px tall)
    let wt_bar_w = (70i32 * state.wt_volume as i32 / 100).max(1);
    fb.fill_rect(48, 9, 70, 2, 0x0000); // track
    fb.fill_rect(48, 9, wt_bar_w, 2, if state.oscillators_active { 0xFFFF } else { 0x8410 });

    // Gap between blocks
    fb.fill_rect(116, 0, 8, 13, 0x0000);

    // Right block: GR status
    let gr_bg: u16 = if state.granular_active { 0x001F } else { 0x2945 }; // blue if ON
    let gr_fg: u16 = 0xFFFF;
    fb.fill_rect(124, 0, 116, 13, gr_bg);
    fb.draw_text(127, 3, "GR", gr_fg, gr_bg);
    let gr_state_str = if state.granular_active { "On " } else { "Off" };
    fb.draw_text(146, 3, gr_state_str, gr_fg, gr_bg);
    let gr_vol_str = format!("{:3}", state.gr_volume);
    fb.draw_text(212, 3, &gr_vol_str, gr_fg, gr_bg);
    let gr_bar_w = (70i32 * state.gr_volume as i32 / 100).max(1);
    fb.fill_rect(172, 9, 70, 2, 0x0000);
    fb.fill_rect(172, 9, gr_bar_w, 2, if state.granular_active { 0xFFFF } else { 0x8410 });

    // ── Key + Scale status line ───────────────────────────────────────────────
    let key_octave = format!("{}{}", state.key_name(), state.octave);
    let scale_name = SCALE_NAMES[state.scale_index];
    let status = format!("{} {}", key_octave, scale_name);
    fb.draw_text(4, 16, &status, 0xFFFF, 0x0000);

    // ── Divider ───────────────────────────────────────────────────────────────
    fb.fill_rect(0, 26, 240, 2, 0x4208);

    // ── Items ─────────────────────────────────────────────────────────────────
    let lines: Vec<String> = vec![
        format!("{:<20}{:>9}", "Wavetable", if state.oscillators_active { "On" } else { "Off" }),
        format!("{:<20}{:>9}", "Granular",  if state.granular_active    { "On" } else { "Off" }),
        format!("{:<20}{:>9}", "Key",       state.key_name()),
        format!("{:<20}{:>9}", "Octave",    state.octave),
        format!("{:<20}{:>9}", "Scale",     SCALE_NAMES[state.scale_index]),
        format!("{:<20}{:>9}", "WT Bank",   BANK_NAMES[state.bank_index]),
        format!("{:<20}{:>9}", "WT Vol",    state.wt_volume),
        format!("{:<20}{:>9}", "GR Vol",    state.gr_volume),
        format!("{:<20}{:>9}", "Stereo",    state.stereo_spread),
        format!("{:<20}{:>9}", "Cents",     format!("{:+}", state.fine_tune_cents as i32)),
        format!("{:<20}{:>9}", "WT Oscs",   state.osc_count),
        format!("{:<20}{:>9}", "GR Voices", state.gr_voices),
        format!("{:<20}{:>9}", "Video",     state.video_status.as_str()),
    ];

    for (index, line) in lines.iter().enumerate() {
        if index >= scroll_offset && index < scroll_offset + VISIBLE_ROWS {
            let visual_row = index - scroll_offset;
            let y = 30 + (visual_row as i32 * 18);
            let selected = index == selected_item;
            let bg = if selected { 0x07E0 } else { 0x0000 };
            let fg = if selected { 0x0000 } else { 0xFFFF };
            fb.fill_rect(2, y - 2, 236, 14, bg);
            fb.draw_text(4, y, line, fg, bg);
        }
    }

    fb.save_ppm(path)
}

/// Render the idle "at-a-glance" overview screen to a PPM file.
pub fn draw_idle_screen_to_ppm(state: &MenuState, path: &Path) -> Result<()> {
    render_idle_screen_to_ppm(state, path)
}

fn render_idle_screen_to_ppm(state: &MenuState, path: &Path) -> Result<()> {
    let mut fb = Framebuffer::new(240, 240);
    fb.clear(0x0000);

    // ── Large key + octave ────────────────────────────────────────────────────
    // Key at 4× (32px per char), centred
    let key = state.key_name(); // e.g. "A", "C#"
    let key_chars = key.chars().count();
    let key_total_w = key_chars as i32 * 32;
    let key_x = (240 - key_total_w) / 2;
    fb.draw_text_4x(key_x, 10, key, 0xFFFF, 0x0000);

    // Octave at 2× right of key
    let octave_str = format!("{}", state.octave);
    let octave_x = key_x + key_total_w + 4;
    fb.draw_text_2x(octave_x, 28, &octave_str, 0x07E0, 0x0000); // green octave

    // ── Scale name at 2× centred ──────────────────────────────────────────────
    let scale = SCALE_NAMES[state.scale_index];
    let scale_w = scale.chars().count() as i32 * 16;
    let scale_x = (240 - scale_w) / 2;
    fb.draw_text_2x(scale_x, 58, scale, 0xAD55, 0x0000); // muted cyan/teal

    // ── Volume bars ───────────────────────────────────────────────────────────
    // Each bar: 50px wide, up to 80px tall, vertical, fills from bottom
    let bar_max_h = 80i32;
    let bar_w = 50i32;
    let bar_top = 90i32; // top of bar area

    // WT bar (left side)
    let wt_color: u16 = if state.oscillators_active { 0x07E0 } else { 0x2945 };
    let wt_bar_h = (bar_max_h * state.wt_volume as i32 / 100).max(2);
    let wt_x = 35i32;
    fb.fill_rect(wt_x, bar_top, bar_w, bar_max_h, 0x1084); // track
    fb.fill_rect(wt_x, bar_top + (bar_max_h - wt_bar_h), bar_w, wt_bar_h, wt_color);
    // Label
    fb.draw_text_2x(wt_x + 9, bar_top + bar_max_h + 4, "WT", if state.oscillators_active { 0x07E0 } else { 0x8410 }, 0x0000);
    // Volume value
    let wt_vol_str = format!("{:3}", state.wt_volume);
    fb.draw_text(wt_x + 9, bar_top + bar_max_h + 22, &wt_vol_str, 0xFFFF, 0x0000);

    // GR bar (right side)
    let gr_color: u16 = if state.granular_active { 0x001F } else { 0x2945 }; // blue if ON
    let gr_bar_h = (bar_max_h * state.gr_volume as i32 / 100).max(2);
    let gr_x = 155i32;
    fb.fill_rect(gr_x, bar_top, bar_w, bar_max_h, 0x1084);
    fb.fill_rect(gr_x, bar_top + (bar_max_h - gr_bar_h), bar_w, gr_bar_h, gr_color);
    fb.draw_text_2x(gr_x + 9, bar_top + bar_max_h + 4, "GR", if state.granular_active { 0x001F } else { 0x8410 }, 0x0000);
    let gr_vol_str = format!("{:3}", state.gr_volume);
    fb.draw_text(gr_x + 9, bar_top + bar_max_h + 22, &gr_vol_str, 0xFFFF, 0x0000);

    // ── Sine wave ─────────────────────────────────────────────────────────────
    let wave_color: u16 = if state.oscillators_active || state.granular_active { 0x4208 } else { 0x2104 };
    let wave_y_center = 195i32;
    for x in 0..240usize {
        let t = x as f32 * std::f32::consts::TAU / 240.0 * 2.5;
        let y_off = (t.sin() * 14.0) as i32;
        let y_px = wave_y_center + y_off;
        if y_px >= 0 && y_px < 239 {
            fb.set_pixel(x, y_px as usize, wave_color);
            fb.set_pixel(x, (y_px + 1) as usize, wave_color);
        }
    }

    // ── "Press any key" ───────────────────────────────────────────────────────
    fb.draw_text(44, 226, "Press any key", 0x4208, 0x0000); // dim white

    fb.save_ppm(path)
}
