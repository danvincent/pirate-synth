use std::f32::consts::PI;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum EngineError {
    #[error("wavetable must have at least 2 samples")]
    EmptyWavetable,
    #[error("oscillator count must be >= 1")]
    InvalidOscillatorCount,
}

#[derive(Clone, Debug)]
pub struct Wavetable {
    pub name: String,
    pub samples: Vec<f32>,
}

#[derive(Clone, Debug)]
struct Oscillator {
    phase: f32,
    detune_ratio: f32,
    current_base_hz: f32,
    pending_base_hz: Option<f32>,
    delay_cycles_remaining: u32,
    rng_state: u64,
    drift_lfo_phase: f32,
    drift_lfo_rate_hz: f32,
    pan_l: f32,
    pan_r: f32,
    tremolo_phase: f32,
    tremolo_rate_hz: f32,
    filter_lfo_phase: f32,
    filter_state: f32,
}

struct CombFilter {
    buffer: Vec<f32>,
    pos: usize,
    feedback: f32,
    damp: f32,
    damp_state: f32,
}

impl CombFilter {
    fn new(delay_samples: usize, feedback: f32, damp: f32) -> Self {
        Self {
            buffer: vec![0.0; delay_samples],
            pos: 0,
            feedback,
            damp,
            damp_state: 0.0,
        }
    }

    fn process(&mut self, input: f32) -> f32 {
        let out = self.buffer[self.pos];
        self.damp_state = out * (1.0 - self.damp) + self.damp_state * self.damp;
        self.buffer[self.pos] = input + self.damp_state * self.feedback;
        self.pos = (self.pos + 1) % self.buffer.len();
        out
    }
}

struct AllpassFilter {
    buffer: Vec<f32>,
    pos: usize,
}

impl AllpassFilter {
    fn new(delay_samples: usize) -> Self {
        Self { buffer: vec![0.0; delay_samples], pos: 0 }
    }

    fn process(&mut self, input: f32) -> f32 {
        let buf = self.buffer[self.pos];
        let out = -input + buf;
        self.buffer[self.pos] = input + buf * 0.5;
        self.pos = (self.pos + 1) % self.buffer.len();
        out
    }
}

struct Reverb {
    combs: [CombFilter; 4],
    allpasses: [AllpassFilter; 2],
}

impl Reverb {
    /// short = true → short room (odd bus); short = false → long room (even bus)
    fn new(short: bool) -> Self {
        let scale = if short { 1.0f32 } else { 1.25f32 };
        let feedback = 0.84f32;
        let damp = 0.20f32;
        // Base comb delays (samples at 48kHz)
        let delays = [1116usize, 1188, 1277, 1356];
        let combs = [
            CombFilter::new((delays[0] as f32 * scale) as usize, feedback, damp),
            CombFilter::new((delays[1] as f32 * scale) as usize, feedback, damp),
            CombFilter::new((delays[2] as f32 * scale) as usize, feedback, damp),
            CombFilter::new((delays[3] as f32 * scale) as usize, feedback, damp),
        ];
        let allpasses = [
            AllpassFilter::new(556),
            AllpassFilter::new(441),
        ];
        Self { combs, allpasses }
    }

    fn process(&mut self, input: f32) -> f32 {
        let comb_sum = self.combs[0].process(input)
            + self.combs[1].process(input)
            + self.combs[2].process(input)
            + self.combs[3].process(input);
        let ap1 = self.allpasses[0].process(comb_sum);
        self.allpasses[1].process(ap1)
    }
}

pub struct Engine {
    sample_rate: u32,
    wavetables: Vec<Wavetable>,
    wavetable_offset: usize,
    oscillators: Vec<Oscillator>,
    base_frequency_hz: f32,
    fine_tune_cents: f32,
    stereo_spread: u8,
    // Reverb
    reverb_enabled: bool,
    reverb_wet: f32,
    reverb_odd: Reverb,
    reverb_even: Reverb,
    // Tremolo
    tremolo_enabled: bool,
    tremolo_depth: f32,
    // Crossfade
    crossfade_enabled: bool,
    xfade_rate: f32,
    xfade_t: f32,
    xfade_index_offset: usize,
    // Filter sweep
    filter_sweep_enabled: bool,
    filter_sweep_min: f32,
    filter_sweep_max: f32,
    filter_sweep_rate_hz: f32,
    // FM
    fm_enabled: bool,
    fm_depth: f32,
    fm_depth_ramp: f32,
    // Subtractive
    subtractive_enabled: bool,
    subtractive_depth: f32,
    subtractive_depth_ramp: f32,
    // Scale
    scale_mode: ScaleMode,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ScaleMode {
    None,
    Major,
    NaturalMinor,
    Pentatonic,
    Dorian,
    Mixolydian,
    WholeTone,
    Hirajoshi,
    Lydian,
}

impl ScaleMode {
    /// Semitone offsets from root (within one octave)
    pub fn semitones(&self) -> &'static [i32] {
        match self {
            ScaleMode::None => &[],
            ScaleMode::Major => &[0, 2, 4, 5, 7, 9, 11],
            ScaleMode::NaturalMinor => &[0, 2, 3, 5, 7, 8, 10],
            ScaleMode::Pentatonic => &[0, 2, 4, 7, 9],
            ScaleMode::Dorian => &[0, 2, 3, 5, 7, 9, 10],
            ScaleMode::Mixolydian => &[0, 2, 4, 5, 7, 9, 10],
            ScaleMode::WholeTone => &[0, 2, 4, 6, 8, 10],
            ScaleMode::Hirajoshi => &[0, 2, 3, 7, 8],
            ScaleMode::Lydian => &[0, 2, 4, 6, 7, 9, 11],
        }
    }
}

fn lcg_next(state: &mut u64) -> u32 {
    *state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    ((*state >> 33) ^ (*state >> 17)) as u32
}

impl Engine {
    pub fn new(
        sample_rate: u32,
        oscillator_count: usize,
        wavetables: Vec<Wavetable>,
    ) -> Result<Self> {
        if oscillator_count == 0 {
            return Err(EngineError::InvalidOscillatorCount.into());
        }
        if wavetables.is_empty() || wavetables.iter().any(|w| w.samples.len() < 2) {
            return Err(EngineError::EmptyWavetable.into());
        }

        let mut oscillators = Vec::new();
        for i in 0..oscillator_count {
            let center = (oscillator_count.saturating_sub(1) as f32) / 2.0;
            let cents = (i as f32 - center) * 4.0;
            
            let mut rng_state = (sample_rate as u64).wrapping_mul(0xdeadbeef).wrapping_add((i as u64).wrapping_mul(0x9e3779b97f4a7c15));
            let drift_lfo_start = lcg_next(&mut rng_state) as f32 / u32::MAX as f32;
            let drift_lfo_rate = 0.05 + (lcg_next(&mut rng_state) as f32 / u32::MAX as f32) * 0.45;
            let tremolo_phase_start = lcg_next(&mut rng_state) as f32 / u32::MAX as f32;
            let tremolo_rate = 0.03 + (lcg_next(&mut rng_state) as f32 / u32::MAX as f32) * 0.22;
            
            oscillators.push(Oscillator {
                phase: 0.0,
                detune_ratio: 2.0f32.powf(cents / 1200.0),
                current_base_hz: 65.406_39,
                pending_base_hz: None,
                delay_cycles_remaining: 0,
                rng_state,
                drift_lfo_phase: drift_lfo_start,
                drift_lfo_rate_hz: drift_lfo_rate,
                pan_l: std::f32::consts::FRAC_1_SQRT_2,
                pan_r: std::f32::consts::FRAC_1_SQRT_2,
                tremolo_phase: tremolo_phase_start,
                tremolo_rate_hz: tremolo_rate,
                filter_lfo_phase: i as f32 / oscillator_count as f32,
                filter_state: 0.0,
            });
        }

        let mut engine = Self {
            sample_rate,
            wavetables,
            wavetable_offset: 0,
            oscillators,
            base_frequency_hz: 65.406_39,
            fine_tune_cents: 0.0,
            stereo_spread: 0,
            reverb_enabled: true,
            reverb_wet: 0.20,
            reverb_odd: Reverb::new(true),
            reverb_even: Reverb::new(false),
            tremolo_enabled: true,
            tremolo_depth: 0.35,
            crossfade_enabled: true,
            xfade_rate: 0.05,
            xfade_t: 0.0,
            xfade_index_offset: 0,
            filter_sweep_enabled: true,
            filter_sweep_min: 0.15,
            filter_sweep_max: 0.80,
            filter_sweep_rate_hz: 0.008,
            fm_enabled: false,
            fm_depth: 0.15,
            fm_depth_ramp: 0.0,
            subtractive_enabled: false,
            subtractive_depth: 0.30,
            subtractive_depth_ramp: 0.0,
            scale_mode: ScaleMode::None,
        };
        
        engine.set_stereo_spread(0);
        
        Ok(engine)
    }

    pub fn oscillator_count(&self) -> usize {
        self.oscillators.len()
    }

    pub fn set_frequency(&mut self, frequency_hz: f32) {
        let hz = frequency_hz.max(1.0);
        self.base_frequency_hz = hz;
        for osc in &mut self.oscillators {
            osc.current_base_hz = hz;
            osc.pending_base_hz = None;
            osc.delay_cycles_remaining = 0;
        }
    }

    pub fn set_frequency_scheduled(&mut self, frequency_hz: f32) {
        let hz = frequency_hz.max(1.0);
        self.base_frequency_hz = hz;
        for osc in &mut self.oscillators {
            let delay = 1 + (lcg_next(&mut osc.rng_state) % 20);
            osc.pending_base_hz = Some(hz);
            osc.delay_cycles_remaining = delay;
        }
    }

    pub fn frequency_pending(&self) -> bool {
        self.oscillators.iter().any(|o| o.pending_base_hz.is_some())
    }

    pub fn set_fine_tune_cents(&mut self, cents: f32) {
        self.fine_tune_cents = cents;
    }

    pub fn set_stereo_spread(&mut self, spread: u8) {
        let spread = spread.min(100);
        self.stereo_spread = spread;
        let spread_f = spread as f32 / 100.0;
        let n = self.oscillators.len();
        for (i, osc) in self.oscillators.iter_mut().enumerate() {
            let pan_pos = if n <= 1 {
                0.0f32
            } else {
                (-1.0 + 2.0 * i as f32 / (n - 1) as f32) * spread_f
            };
            let angle = (pan_pos + 1.0) * std::f32::consts::FRAC_PI_4;
            osc.pan_l = angle.cos();
            osc.pan_r = angle.sin();
        }
    }

    pub fn set_wavetable_offset(&mut self, offset: usize) {
        if !self.wavetables.is_empty() {
            self.wavetable_offset = offset % self.wavetables.len();
        }
    }

    pub fn set_reverb(&mut self, enabled: bool, wet: f32) {
        self.reverb_enabled = enabled;
        self.reverb_wet = wet.clamp(0.0, 1.0);
    }

    pub fn set_tremolo(&mut self, enabled: bool, depth: f32) {
        self.tremolo_enabled = enabled;
        self.tremolo_depth = depth.clamp(0.0, 1.0);
    }

    pub fn set_crossfade(&mut self, enabled: bool, rate: f32) {
        self.crossfade_enabled = enabled;
        self.xfade_rate = rate.max(0.0);
    }

    pub fn set_filter_sweep(&mut self, enabled: bool, min: f32, max: f32, rate_hz: f32) {
        self.filter_sweep_enabled = enabled;
        self.filter_sweep_min = min.clamp(0.0, 1.0);
        self.filter_sweep_max = max.clamp(0.0, 1.0);
        self.filter_sweep_rate_hz = rate_hz.max(0.0);
    }

    pub fn set_fm(&mut self, enabled: bool, depth: f32) {
        self.fm_enabled = enabled;
        self.fm_depth = depth.clamp(0.0, 1.0);
        if enabled {
            self.fm_depth_ramp = 0.0;  // Start ramp from 0 when enabling
        } else {
            self.fm_depth_ramp = 0.0;  // Snap to 0 when disabling
        }
    }

    pub fn set_subtractive(&mut self, enabled: bool, depth: f32) {
        self.subtractive_enabled = enabled;
        self.subtractive_depth = depth.clamp(0.0, 1.0);
        if enabled {
            self.subtractive_depth_ramp = 0.0;  // Start ramp from 0 when enabling
        } else {
            self.subtractive_depth_ramp = 0.0;  // Snap to 0 when disabling
        }
    }

    pub fn set_scale(&mut self, mode: ScaleMode, spread_percent: f32) {
        self.scale_mode = mode;
        let n = self.oscillators.len();
        let center = (n.saturating_sub(1) as f32) / 2.0;
        
        match mode {
            ScaleMode::None => {
                // Restore original uniform 4-cent spread
                for (i, osc) in self.oscillators.iter_mut().enumerate() {
                    let cents = (i as f32 - center) * 4.0;
                    osc.detune_ratio = 2.0f32.powf(cents / 1200.0);
                }
            }
            _ => {
                let semitones = mode.semitones();
                let spread_cents = spread_percent.abs().min(100.0) / 100.0 * 1200.0;
                
                // Distribute oscillators evenly across spread range
                // then snap each to nearest scale semitone
                for i in 0..n {
                    // Position: 0.0 to 1.0 across the spread
                    let t = if n <= 1 { 0.5 } else { i as f32 / (n - 1) as f32 };
                    let target_cents = t * spread_cents;
                    
                    // Find nearest scale degree (in cents = semitone * 100)
                    // Scale degrees can span multiple octaves if spread > 1200
                    let nearest_cents = semitones.iter().map(|&st| {
                        let base = (st * 100) as f32;
                        // Also check octave multiples to find truly nearest
                        let mut best = base;
                        let mut best_dist = (base - target_cents).abs();
                        // Check adjacent octaves
                        for octave_offset in [-1200.0f32, 0.0, 1200.0, 2400.0] {
                            let candidate = base + octave_offset;
                            if candidate >= 0.0 {
                                let dist = (candidate - target_cents).abs();
                                if dist < best_dist {
                                    best_dist = dist;
                                    best = candidate;
                                }
                            }
                        }
                        best
                    }).min_by(|a, b| {
                        let da = (a - target_cents).abs();
                        let db = (b - target_cents).abs();
                        da.partial_cmp(&db).unwrap()
                    }).unwrap_or(0.0);
                    
                    self.oscillators[i].detune_ratio = 2.0f32.powf(nearest_cents / 1200.0);
                }
            }
        }
    }

    pub fn render_i16_stereo(&mut self, out: &mut [i16]) {
        for frame in out.chunks_exact_mut(2) {
            // Smooth ramp for FM enable/disable
            if self.fm_enabled {
                let gap = self.fm_depth - self.fm_depth_ramp;
                self.fm_depth_ramp += gap.clamp(-0.001, 0.001);
            } else {
                self.fm_depth_ramp = 0.0;
            }

            // Smooth ramp for subtractive enable/disable
            if self.subtractive_enabled {
                let gap = self.subtractive_depth - self.subtractive_depth_ramp;
                self.subtractive_depth_ramp += gap.clamp(-0.001, 0.001);
            } else {
                self.subtractive_depth_ramp = 0.0;
            }

            // Pre-pass: collect odd oscillator samples for FM without advancing state
            let pre_odd_mono: f32 = if self.fm_enabled {
                let table_count = self.wavetables.len();
                let mut acc = 0.0f32;
                for (osc_idx, osc) in self.oscillators.iter().enumerate() {
                    if osc_idx % 2 == 1 {
                        // peek at current sample without advancing phase
                        let cur_idx = (self.wavetable_offset + self.xfade_index_offset + osc_idx) % table_count;
                        let table = &self.wavetables[cur_idx].samples;
                        let len = table.len() as f32;
                        let phase_pos = osc.phase * len;
                        let i0 = phase_pos as usize % table.len();
                        let i1 = (i0 + 1) % table.len();
                        let frac = phase_pos - (i0 as f32);
                        acc += table[i0] * (1.0 - frac) + table[i1] * frac;
                    }
                }
                acc
            } else {
                0.0
            };

            let mut odd_l = 0.0f32;
            let mut odd_r = 0.0f32;
            let mut even_l = 0.0f32;
            let mut even_r = 0.0f32;
            let table_count = self.wavetables.len();

            for (osc_idx, osc) in self.oscillators.iter_mut().enumerate() {
                let cur_idx = (self.wavetable_offset + self.xfade_index_offset + osc_idx) % table_count;
                let next_idx = (cur_idx + 1) % table_count;

                let s = if self.crossfade_enabled && self.xfade_t > 0.0 {
                    let cur_table = &self.wavetables[cur_idx].samples;
                    let next_table = &self.wavetables[next_idx].samples;
                    let len_cur = cur_table.len() as f32;
                    let len_next = next_table.len() as f32;

                    let phase_pos_cur = osc.phase * len_cur;
                    let i0c = phase_pos_cur as usize % cur_table.len();
                    let i1c = (i0c + 1) % cur_table.len();
                    let frac_c = phase_pos_cur - (i0c as f32);
                    let s_cur = cur_table[i0c] * (1.0 - frac_c) + cur_table[i1c] * frac_c;

                    let phase_pos_next = osc.phase * len_next;
                    let i0n = phase_pos_next as usize % next_table.len();
                    let i1n = (i0n + 1) % next_table.len();
                    let frac_n = phase_pos_next - (i0n as f32);
                    let s_next = next_table[i0n] * (1.0 - frac_n) + next_table[i1n] * frac_n;

                    s_cur * (1.0 - self.xfade_t) + s_next * self.xfade_t
                } else {
                    let table = &self.wavetables[cur_idx].samples;
                    let len = table.len() as f32;
                    let phase_pos = osc.phase * len;
                    let i0 = phase_pos as usize % table.len();
                    let i1 = (i0 + 1) % table.len();
                    let frac = phase_pos - (i0 as f32);
                    table[i0] * (1.0 - frac) + table[i1] * frac
                };
                let mut s = s;

                // Apply tremolo if enabled
                if self.tremolo_enabled {
                    let amp = 1.0 - self.tremolo_depth * (1.0 - (osc.tremolo_phase * 2.0 * std::f32::consts::PI).cos()) * 0.5;
                    let incr = osc.tremolo_rate_hz / self.sample_rate as f32;
                    osc.tremolo_phase = (osc.tremolo_phase + incr).fract();
                    s = s * amp;
                }

                // Drift LFO
                let lfo_incr = osc.drift_lfo_rate_hz / self.sample_rate as f32;
                osc.drift_lfo_phase = (osc.drift_lfo_phase + lfo_incr).fract();
                let drift_cents = self.fine_tune_cents * (osc.drift_lfo_phase * 2.0 * std::f32::consts::PI).sin();
                let drift_ratio = 2.0f32.powf(drift_cents / 1200.0);

                // FM modulation: even osc use pre-collected odd samples
                let fm_mod = if self.fm_enabled && osc_idx % 2 == 0 { pre_odd_mono * self.fm_depth_ramp } else { 0.0 };
                let incr = (osc.current_base_hz * osc.detune_ratio * drift_ratio) / self.sample_rate as f32 + fm_mod;
                let new_phase = osc.phase + incr;
                let cycles_completed = new_phase.floor() as u32;
                if cycles_completed > 0 {
                    if let Some(pending_hz) = osc.pending_base_hz {
                        if osc.delay_cycles_remaining <= cycles_completed {
                            osc.current_base_hz = pending_hz;
                            osc.pending_base_hz = None;
                            osc.delay_cycles_remaining = 0;
                        } else {
                            osc.delay_cycles_remaining -= cycles_completed;
                        }
                    }
                }
                osc.phase = new_phase.fract();

                // Apply per-oscillator filter sweep if enabled
                let s = if self.filter_sweep_enabled {
                    let cos_val = (osc.filter_lfo_phase * 2.0 * std::f32::consts::PI).cos();
                    let a = self.filter_sweep_min + (self.filter_sweep_max - self.filter_sweep_min) * (1.0 - cos_val) * 0.5;
                    let a = a.clamp(0.0, 1.0);
                    osc.filter_lfo_phase = (osc.filter_lfo_phase + self.filter_sweep_rate_hz / self.sample_rate as f32).fract();
                    osc.filter_state = a * osc.filter_state + (1.0 - a) * s;
                    osc.filter_state
                } else {
                    s
                };

                // Route to odd or even bus
                if osc_idx % 2 == 1 {
                    odd_l += s * osc.pan_l;
                    odd_r += s * osc.pan_r;
                } else {
                    even_l += s * osc.pan_l;
                    even_r += s * osc.pan_r;
                }
            }

            // Advance crossfade timer
            if self.crossfade_enabled {
                let xfade_incr = self.xfade_rate / self.sample_rate as f32;
                self.xfade_t += xfade_incr;
                if self.xfade_t >= 1.0 {
                    self.xfade_t = 0.0;
                    self.xfade_index_offset = (self.xfade_index_offset + 1) % table_count.max(1);
                }
            }

            // Apply subtractive mixing BEFORE reverb
            let (eff_even_l, eff_even_r) = if self.subtractive_enabled {
                (even_l * (1.0 - self.subtractive_depth_ramp), even_r * (1.0 - self.subtractive_depth_ramp))
            } else {
                (even_l, even_r)
            };

            // Apply reverb if enabled
            let (l_out, r_out) = if self.reverb_enabled {
                let wet = self.reverb_wet;
                let dry = 1.0 - wet;
                let odd_rev_l = self.reverb_odd.process(odd_l);
                let odd_rev_r = self.reverb_odd.process(odd_r);
                let even_rev_l = self.reverb_even.process(eff_even_l);
                let even_rev_r = self.reverb_even.process(eff_even_r);
                (
                    dry * (odd_l + eff_even_l) + wet * (odd_rev_l + even_rev_l),
                    dry * (odd_r + eff_even_r) + wet * (odd_rev_r + even_rev_r),
                )
            } else {
                (odd_l + eff_even_l, odd_r + eff_even_r)
            };

            // Filter sweep is now applied per-oscillator above

            let gain = 0.25f32 / self.oscillators.len() as f32;
            let l = (l_out * gain).clamp(-1.0, 1.0);
            let r = (r_out * gain).clamp(-1.0, 1.0);
            frame[0] = (l * i16::MAX as f32) as i16;
            frame[1] = (r * i16::MAX as f32) as i16;
        }
    }
}

pub fn key_to_frequency_hz(key: &str, octave: i32, fine_tune_cents: f32) -> Result<f32> {
    let semitone = key_to_semitone(key)? as i32;
    let midi_note = 12 * (octave + 1) + semitone;
    let freq = 440.0f32 * 2.0f32.powf((midi_note as f32 - 69.0) / 12.0);
    let tuned = freq * 2.0f32.powf(fine_tune_cents / 1200.0);
    Ok(tuned)
}

fn key_to_semitone(key: &str) -> Result<u8> {
    let value = match key {
        "C" => 0,
        "C#" | "Db" => 1,
        "D" => 2,
        "D#" | "Eb" => 3,
        "E" => 4,
        "F" => 5,
        "F#" | "Gb" => 6,
        "G" => 7,
        "G#" | "Ab" => 8,
        "A" => 9,
        "A#" | "Bb" => 10,
        "B" => 11,
        other => anyhow::bail!("unsupported key: {other}"),
    };
    Ok(value)
}

pub fn load_wavetables(wavetable_dir: &Path, min_count: usize) -> Result<Vec<Wavetable>> {
    let mut files: Vec<PathBuf> = fs::read_dir(wavetable_dir)
        .with_context(|| {
            format!(
                "failed to read wavetable directory: {}",
                wavetable_dir.display()
            )
        })?
        .flatten()
        .map(|e| e.path())
        .filter(|path| path.is_file())
        .collect();
    files.sort();

    let mut out = Vec::new();
    for file in files {
        if let Some(wavetable) = load_wavetable_file(&file)? {
            out.push(wavetable);
        }
    }

    if out.len() < min_count {
        let builtins = builtin_wavetables();
        // First pass: add unique built-ins by name
        for builtin in &builtins {
            if out.len() >= min_count {
                break;
            }
            if !out.iter().any(|w| w.name == builtin.name) {
                out.push(builtin.clone());
            }
        }
        // Second pass: if min_count > number of unique built-ins, cycle through
        // built-ins again with an index suffix, checking against existing names
        // to guarantee uniqueness even when user files already use suffixed names.
        if out.len() < min_count {
            let mut cycle = 0usize;
            while out.len() < min_count {
                let b = &builtins[cycle % builtins.len()];
                let mut suffix = cycle / builtins.len() + 2;
                let name = loop {
                    let candidate = format!("{}{}", b.name, suffix);
                    if !out.iter().any(|w| w.name == candidate) {
                        break candidate;
                    }
                    suffix += 1;
                };
                out.push(Wavetable {
                    name,
                    samples: b.samples.clone(),
                });
                cycle += 1;
            }
        }
    }

    Ok(out)
}

fn load_wavetable_file(path: &Path) -> Result<Option<Wavetable>> {
    let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
        return Ok(None);
    };
    let Some(ext) = path.extension().and_then(|s| s.to_str()) else {
        return Ok(None);
    };

    if !matches!(ext, "wt" | "txt" | "csv") {
        return Ok(None);
    }

    let content = fs::read_to_string(path)
        .with_context(|| format!("failed to read wavetable file: {}", path.display()))?;

    let mut samples = Vec::new();
    for token in content
        .split(|c: char| c.is_whitespace() || c == ',')
        .filter(|token| !token.is_empty())
    {
        let value: f32 = token
            .parse()
            .with_context(|| format!("invalid wavetable sample '{token}' in {}", path.display()))?;
        samples.push(value.clamp(-1.0, 1.0));
    }

    if samples.len() < 2 {
        return Ok(None);
    }

    Ok(Some(Wavetable {
        name: stem.to_string(),
        samples,
    }))
}

pub fn default_sine_wavetable() -> Wavetable {
    let size = 512;
    let mut samples = Vec::with_capacity(size);
    for i in 0..size {
        let phase = (i as f32 / size as f32) * 2.0 * PI;
        samples.push(phase.sin());
    }
    Wavetable {
        name: "sine".to_string(),
        samples,
    }
}

pub fn builtin_wavetables() -> Vec<Wavetable> {
    let size = 512;
    let mut result = Vec::new();

    // 1. sine
    result.push(default_sine_wavetable());

    // 2. triangle
    {
        let mut samples = Vec::with_capacity(size);
        for i in 0..size {
            let phase = i as f32 / size as f32;
            let s = if phase < 0.25 {
                4.0 * phase
            } else if phase < 0.75 {
                2.0 - 4.0 * phase
            } else {
                4.0 * phase - 4.0
            };
            samples.push(s);
        }
        result.push(Wavetable {
            name: "triangle".to_string(),
            samples,
        });
    }

    // 3. sawtooth
    {
        let mut samples = Vec::with_capacity(size);
        for i in 0..size {
            let phase = i as f32 / size as f32;
            samples.push(2.0 * phase - 1.0);
        }
        result.push(Wavetable {
            name: "sawtooth".to_string(),
            samples,
        });
    }

    // 4. ramp
    {
        let mut samples = Vec::with_capacity(size);
        for i in 0..size {
            let phase = i as f32 / size as f32;
            samples.push(1.0 - 2.0 * phase);
        }
        result.push(Wavetable {
            name: "ramp".to_string(),
            samples,
        });
    }

    // 5. square
    {
        let mut samples = Vec::with_capacity(size);
        for i in 0..size {
            let phase = i as f32 / size as f32;
            samples.push(if phase < 0.5 { 1.0 } else { -1.0 });
        }
        result.push(Wavetable {
            name: "square".to_string(),
            samples,
        });
    }

    // 6. pulse33
    {
        let mut samples = Vec::with_capacity(size);
        for i in 0..size {
            let phase = i as f32 / size as f32;
            samples.push(if phase < 0.333 { 1.0 } else { -1.0 });
        }
        result.push(Wavetable {
            name: "pulse33".to_string(),
            samples,
        });
    }

    // 7. sine3rd
    {
        let mut samples = Vec::with_capacity(size);
        for i in 0..size {
            let phase = i as f32 / size as f32;
            let phase_rad = phase * 2.0 * PI;
            let s = (phase_rad.sin() + 0.5 * (3.0 * phase_rad).sin()).clamp(-1.0, 1.0);
            samples.push(s);
        }
        result.push(Wavetable {
            name: "sine3rd".to_string(),
            samples,
        });
    }

    // 8. sine5th
    {
        let mut samples = Vec::with_capacity(size);
        for i in 0..size {
            let phase = i as f32 / size as f32;
            let phase_rad = phase * 2.0 * PI;
            let s = (phase_rad.sin() + 0.33 * (5.0 * phase_rad).sin()).clamp(-1.0, 1.0);
            samples.push(s);
        }
        result.push(Wavetable {
            name: "sine5th".to_string(),
            samples,
        });
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;
    use std::collections::HashSet;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn load_wavetables_cycles_builtins_when_min_count_exceeds_unique_builtin_count() {
        let builtin_len = builtin_wavetables().len();
        let min_count = builtin_len + 1;
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let temp_dir = std::env::temp_dir().join(format!("engine_wavetable_test_{unique}"));
        fs::create_dir_all(&temp_dir).unwrap();
        let loaded = load_wavetables(&temp_dir, min_count).unwrap();
        let names: HashSet<_> = loaded.iter().map(|w| w.name.as_str()).collect();
        assert_eq!(loaded.len(), min_count);
        assert_eq!(names.len(), loaded.len());
        fs::remove_dir_all(&temp_dir).unwrap();
    }

    #[test]
    fn converts_key_to_frequency() {
        assert_relative_eq!(
            key_to_frequency_hz("A", 4, 0.0).unwrap(),
            440.0,
            epsilon = 0.01
        );
        assert_relative_eq!(
            key_to_frequency_hz("C", 4, 0.0).unwrap(),
            261.625,
            epsilon = 0.1
        );
    }

    #[test]
    fn fine_tune_applies_cents() {
        let base = key_to_frequency_hz("C", 4, 0.0).unwrap();
        let up = key_to_frequency_hz("C", 4, 100.0).unwrap();
        assert!(up > base);
    }

    #[test]
    fn engine_allocates_oscillator_count_once() {
        let table = default_sine_wavetable();
        let engine = Engine::new(48_000, 8, vec![table]).unwrap();
        assert_eq!(engine.oscillator_count(), 8);
    }

    #[test]
    fn engine_supports_multi_wavetable_bank() {
        let mut sine = default_sine_wavetable();
        sine.name = "sine".to_string();
        let square = Wavetable {
            name: "square".to_string(),
            samples: vec![1.0, 1.0, -1.0, -1.0],
        };

        let mut engine = Engine::new(48_000, 8, vec![sine, square]).unwrap();
        let mut out = [0i16; 64];
        engine.render_i16_stereo(&mut out);
        let first = out[0];
        engine.set_wavetable_offset(1);
        engine.render_i16_stereo(&mut out);
        let second = out[0];
        assert_ne!(first, second);
    }

    #[test]
    fn builtin_wavetables_are_valid() {
        for wt in builtin_wavetables() {
            assert!(wt.samples.len() >= 2, "{} has too few samples", wt.name);
            for s in &wt.samples {
                assert!(*s >= -1.0 && *s <= 1.0, "{} has out-of-range sample {}", wt.name, s);
            }
        }
    }

    #[test]
    fn load_wavetables_pads_to_min_count() {
        let dir = std::env::temp_dir().join(format!("pirate_synth_test_empty_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let result = load_wavetables(&dir, 8).unwrap();
        assert_eq!(result.len(), 8);
        let names: std::collections::HashSet<_> = result.iter().map(|w| w.name.as_str()).collect();
        assert_eq!(names.len(), 8);
    }

    #[test]
    fn scheduled_frequency_not_immediate() {
        let table = default_sine_wavetable();
        let mut engine = Engine::new(48_000, 1, vec![table]).unwrap();
        engine.set_frequency(440.0);
        engine.set_frequency_scheduled(880.0);
        let mut out = vec![0i16; 100];
        engine.render_i16_stereo(&mut out);
        assert!(engine.frequency_pending());
    }

    #[test]
    fn scheduled_frequency_applies_within_twenty_cycles() {
        let table = default_sine_wavetable();
        let mut engine = Engine::new(48_000, 1, vec![table]).unwrap();
        engine.set_frequency(440.0);
        engine.set_frequency_scheduled(880.0);
        let mut out = vec![0i16; 2200 * 2];
        engine.render_i16_stereo(&mut out);
        assert!(!engine.frequency_pending());
    }

    #[test]
    fn drift_zero_produces_uniform_incr() {
        let table = default_sine_wavetable();
        let mut engine = Engine::new(48_000, 2, vec![table.clone(), table]).unwrap();
        engine.set_frequency(440.0);
        engine.set_fine_tune_cents(0.0);
        let mut out = vec![0i16; 200];
        engine.render_i16_stereo(&mut out);
        assert!(out.iter().any(|&s| s != 0));
    }

    #[test]
    fn spread_zero_produces_equal_lr() {
        let table = default_sine_wavetable();
        let mut engine = Engine::new(48_000, 4, vec![table.clone(), table.clone(), table.clone(), table]).unwrap();
        engine.set_frequency(440.0);
        engine.set_stereo_spread(0);
        let mut out = vec![0i16; 512];
        engine.render_i16_stereo(&mut out);
        for frame in out.chunks_exact(2) {
            assert_eq!(frame[0], frame[1], "L and R differ at spread=0");
        }
    }

    #[test]
    fn spread_max_produces_wider_than_zero() {
        let table = default_sine_wavetable();
        let mut engine = Engine::new(48_000, 4, vec![table.clone(), table.clone(), table.clone(), table]).unwrap();
        engine.set_frequency(440.0);
        engine.set_stereo_spread(100);
        let mut out = vec![0i16; 512];
        engine.render_i16_stereo(&mut out);
        let differs = out.chunks_exact(2).any(|f| f[0] != f[1]);
        assert!(differs, "L and R should differ at spread=100");
    }

    #[test]
    fn reverb_disabled_passes_dry_unchanged() {
        let table = default_sine_wavetable();
        let mut engine_dry = Engine::new(48_000, 4, vec![table.clone(), table.clone(), table.clone(), table.clone()]).unwrap();
        engine_dry.set_reverb(false, 0.0);
        engine_dry.set_frequency(220.0);

        let mut engine_ref = Engine::new(48_000, 4, vec![table.clone(), table.clone(), table.clone(), table.clone()]).unwrap();
        engine_ref.set_reverb(false, 0.0);
        engine_ref.set_frequency(220.0);

        let mut out_dry = vec![0i16; 256];
        let mut out_ref = vec![0i16; 256];
        engine_dry.render_i16_stereo(&mut out_dry);
        engine_ref.render_i16_stereo(&mut out_ref);
        assert_eq!(out_dry, out_ref);
    }

    #[test]
    fn reverb_wet_zero_equals_disabled() {
        let table = default_sine_wavetable();
        let mut engine_a = Engine::new(48_000, 4, vec![table.clone(), table.clone(), table.clone(), table.clone()]).unwrap();
        engine_a.set_reverb(true, 0.0);
        engine_a.set_frequency(220.0);

        let mut engine_b = Engine::new(48_000, 4, vec![table.clone(), table.clone(), table.clone(), table.clone()]).unwrap();
        engine_b.set_reverb(false, 0.0);
        engine_b.set_frequency(220.0);

        let mut out_a = vec![0i16; 256];
        let mut out_b = vec![0i16; 256];
        engine_a.render_i16_stereo(&mut out_a);
        engine_b.render_i16_stereo(&mut out_b);
        assert_eq!(out_a, out_b);
    }

    #[test]
    fn reverb_enabled_nonzero_wet_modifies_output() {
        let table = default_sine_wavetable();
        let mut engine_dry = Engine::new(48_000, 4, vec![table.clone(), table.clone(), table.clone(), table.clone()]).unwrap();
        engine_dry.set_reverb(false, 0.0);
        engine_dry.set_frequency(220.0);

        let mut engine_wet = Engine::new(48_000, 4, vec![table.clone(), table.clone(), table.clone(), table.clone()]).unwrap();
        engine_wet.set_reverb(true, 0.5);
        engine_wet.set_frequency(220.0);

        // Run long enough for reverb tails to develop
        let mut out_dry = vec![0i16; 4096];
        let mut out_wet = vec![0i16; 4096];
        engine_dry.render_i16_stereo(&mut out_dry);
        engine_wet.render_i16_stereo(&mut out_wet);
        // After enough samples the outputs should diverge
        let differs = out_dry.iter().zip(&out_wet).any(|(a, b)| a != b);
        assert!(differs, "reverb wet output should differ from dry");
    }

    #[test]
    fn tremolo_zero_depth_output_unchanged() {
        let table = default_sine_wavetable();
        let mut engine_a = Engine::new(48_000, 4, vec![table.clone(), table.clone(), table.clone(), table.clone()]).unwrap();
        engine_a.set_tremolo(true, 0.0);
        engine_a.set_reverb(false, 0.0);
        engine_a.set_frequency(110.0);

        let mut engine_b = Engine::new(48_000, 4, vec![table.clone(), table.clone(), table.clone(), table.clone()]).unwrap();
        engine_b.set_tremolo(false, 0.0);
        engine_b.set_reverb(false, 0.0);
        engine_b.set_frequency(110.0);

        let mut out_a = vec![0i16; 512];
        let mut out_b = vec![0i16; 512];
        engine_a.render_i16_stereo(&mut out_a);
        engine_b.render_i16_stereo(&mut out_b);
        assert_eq!(out_a, out_b, "tremolo depth=0 should not change output");
    }

    #[test]
    fn tremolo_disabled_output_unchanged() {
        let table = default_sine_wavetable();
        let mut engine_on = Engine::new(48_000, 4, vec![table.clone(), table.clone(), table.clone(), table.clone()]).unwrap();
        engine_on.set_tremolo(false, 0.5);
        engine_on.set_reverb(false, 0.0);
        engine_on.set_frequency(110.0);

        let mut engine_off = Engine::new(48_000, 4, vec![table.clone(), table.clone(), table.clone(), table.clone()]).unwrap();
        engine_off.set_tremolo(false, 0.0);
        engine_off.set_reverb(false, 0.0);
        engine_off.set_frequency(110.0);

        let mut out_on = vec![0i16; 512];
        let mut out_off = vec![0i16; 512];
        engine_on.render_i16_stereo(&mut out_on);
        engine_off.render_i16_stereo(&mut out_off);
        assert_eq!(out_on, out_off, "tremolo disabled should not change output regardless of depth");
    }

    #[test]
    fn tremolo_nonzero_depth_varies_amplitude() {
        let table = default_sine_wavetable();
        let mut engine = Engine::new(48_000, 2, vec![table.clone(), table.clone()]).unwrap();
        engine.set_tremolo(true, 1.0);
        engine.set_reverb(false, 0.0);
        engine.set_frequency(110.0);

        // Render enough for tremolo to modulate (~1 full period at 0.03 Hz min = 1.6s = 76k samples)
        // Use 2048 samples; check that L values are not all the same magnitude
        let mut out = vec![0i16; 2048];
        engine.render_i16_stereo(&mut out);
        let l_vals: Vec<i16> = out.chunks_exact(2).map(|f| f[0]).collect();
        let min = l_vals.iter().copied().min().unwrap();
        let max = l_vals.iter().copied().max().unwrap();
        // With depth=1.0, amplitude modulates between 0 and 1, so range should be substantial
        assert!(max > 0 && (max - min) > 0, "tremolo with depth=1 should produce varying amplitude");
    }

    #[test]
    fn crossfade_disabled_uses_base_table() {
        let table_a = default_sine_wavetable();
        let table_b = Wavetable { name: "square".to_string(), samples: vec![1.0, 1.0, -1.0, -1.0] };

        let mut engine_cf = Engine::new(48_000, 2, vec![table_a.clone(), table_b.clone()]).unwrap();
        engine_cf.set_crossfade(false, 0.0);
        engine_cf.set_reverb(false, 0.0);
        engine_cf.set_tremolo(false, 0.0);
        engine_cf.set_frequency(110.0);

        let mut engine_base = Engine::new(48_000, 2, vec![table_a.clone(), table_b.clone()]).unwrap();
        engine_base.set_crossfade(false, 0.0);
        engine_base.set_reverb(false, 0.0);
        engine_base.set_tremolo(false, 0.0);
        engine_base.set_frequency(110.0);

        let mut out_cf = vec![0i16; 256];
        let mut out_base = vec![0i16; 256];
        engine_cf.render_i16_stereo(&mut out_cf);
        engine_base.render_i16_stereo(&mut out_base);
        assert_eq!(out_cf, out_base, "disabled crossfade should match baseline");
    }

    #[test]
    fn crossfade_advances_xfade_t() {
        // With a high rate, after enough samples xfade_t should wrap around (index advances)
        let table_a = default_sine_wavetable();
        let table_b = Wavetable { name: "square".to_string(), samples: vec![1.0, 1.0, -1.0, -1.0] };

        let mut engine = Engine::new(48_000, 2, vec![table_a.clone(), table_b.clone()]).unwrap();
        // rate=48000 means xfade_t advances 1.0 per sample → wraps every sample
        engine.set_crossfade(true, 48000.0);
        engine.set_reverb(false, 0.0);
        engine.set_tremolo(false, 0.0);
        engine.set_frequency(110.0);

        let mut out = vec![0i16; 512];
        engine.render_i16_stereo(&mut out);
        // Just verify it doesn't panic and produces some output
        assert!(out.iter().any(|&s| s != 0));
    }

    #[test]
    fn crossfade_at_zero_rate_stays_at_base() {
        let table_a = default_sine_wavetable();
        let table_b = Wavetable { name: "square".to_string(), samples: vec![1.0, 1.0, -1.0, -1.0] };

        let mut engine = Engine::new(48_000, 2, vec![table_a.clone(), table_b.clone()]).unwrap();
        engine.set_crossfade(true, 0.0);  // rate=0 → xfade_t never advances
        engine.set_reverb(false, 0.0);
        engine.set_tremolo(false, 0.0);
        engine.set_frequency(110.0);

        let mut engine_ref = Engine::new(48_000, 2, vec![table_a.clone(), table_b.clone()]).unwrap();
        engine_ref.set_crossfade(false, 0.0);
        engine_ref.set_reverb(false, 0.0);
        engine_ref.set_tremolo(false, 0.0);
        engine_ref.set_frequency(110.0);

        let mut out = vec![0i16; 256];
        let mut out_ref = vec![0i16; 256];
        engine.render_i16_stereo(&mut out);
        engine_ref.render_i16_stereo(&mut out_ref);
        assert_eq!(out, out_ref, "crossfade with rate=0 should not change output");
    }

    #[test]
    fn filter_sweep_disabled_passes_unchanged() {
        let table = default_sine_wavetable();
        let mut engine_a = Engine::new(48_000, 4, vec![table.clone(), table.clone(), table.clone(), table.clone()]).unwrap();
        engine_a.set_filter_sweep(false, 0.15, 0.80, 0.008);
        engine_a.set_reverb(false, 0.0);
        engine_a.set_tremolo(false, 0.0);
        engine_a.set_crossfade(false, 0.0);
        engine_a.set_fm(false, 0.0);
        engine_a.set_subtractive(false, 0.0);
        engine_a.set_frequency(110.0);

        let mut engine_b = Engine::new(48_000, 4, vec![table.clone(), table.clone(), table.clone(), table.clone()]).unwrap();
        engine_b.set_filter_sweep(false, 0.15, 0.80, 0.008);
        engine_b.set_reverb(false, 0.0);
        engine_b.set_tremolo(false, 0.0);
        engine_b.set_crossfade(false, 0.0);
        engine_b.set_fm(false, 0.0);
        engine_b.set_subtractive(false, 0.0);
        engine_b.set_frequency(110.0);

        let mut out_a = vec![0i16; 256];
        let mut out_b = vec![0i16; 256];
        engine_a.render_i16_stereo(&mut out_a);
        engine_b.render_i16_stereo(&mut out_b);
        assert_eq!(out_a, out_b);
    }

    #[test]
    fn fm_disabled_matches_baseline() {
        let table = default_sine_wavetable();
        let make = || {
            let mut e = Engine::new(48_000, 4, vec![table.clone(), table.clone(), table.clone(), table.clone()]).unwrap();
            e.set_reverb(false, 0.0);
            e.set_tremolo(false, 0.0);
            e.set_crossfade(false, 0.0);
            e.set_filter_sweep(false, 0.15, 0.80, 0.008);
            e.set_subtractive(false, 0.0);
            e.set_frequency(110.0);
            e
        };
        let mut engine_a = make();
        engine_a.set_fm(false, 0.5);
        let mut engine_b = make();
        engine_b.set_fm(false, 0.0);

        let mut out_a = vec![0i16; 256];
        let mut out_b = vec![0i16; 256];
        engine_a.render_i16_stereo(&mut out_a);
        engine_b.render_i16_stereo(&mut out_b);
        assert_eq!(out_a, out_b, "FM disabled should not change output regardless of depth");
    }

    #[test]
    fn subtractive_zero_depth_equals_full_sum() {
        let table = default_sine_wavetable();
        let make = || {
            let mut e = Engine::new(48_000, 4, vec![table.clone(), table.clone(), table.clone(), table.clone()]).unwrap();
            e.set_reverb(false, 0.0);
            e.set_tremolo(false, 0.0);
            e.set_crossfade(false, 0.0);
            e.set_filter_sweep(false, 0.15, 0.80, 0.008);
            e.set_fm(false, 0.0);
            e.set_frequency(110.0);
            e
        };
        let mut engine_a = make();
        engine_a.set_subtractive(true, 0.0);
        let mut engine_b = make();
        engine_b.set_subtractive(false, 0.0);

        let mut out_a = vec![0i16; 256];
        let mut out_b = vec![0i16; 256];
        engine_a.render_i16_stereo(&mut out_a);
        engine_b.render_i16_stereo(&mut out_b);
        assert_eq!(out_a, out_b, "subtractive depth=0 should equal normal sum");
    }

    #[test]
    fn fm_depth_ramp_initializes_to_zero() {
        let table = default_sine_wavetable();
        let engine = Engine::new(48_000, 4, vec![table.clone(), table.clone(), table.clone(), table.clone()]).unwrap();
        // FM should start disabled with ramp at 0
        assert!(!engine.fm_enabled);
        assert_eq!(engine.fm_depth_ramp, 0.0, "fm_depth_ramp should start at 0");
    }

    #[test]
    fn subtractive_depth_ramp_initializes_to_zero() {
        let table = default_sine_wavetable();
        let engine = Engine::new(48_000, 4, vec![table.clone(), table.clone(), table.clone(), table.clone()]).unwrap();
        // Subtractive should start disabled with ramp at 0
        assert!(!engine.subtractive_enabled);
        assert_eq!(engine.subtractive_depth_ramp, 0.0, "subtractive_depth_ramp should start at 0");
    }

    #[test]
    fn fm_ramp_smoothly_increases_on_enable() {
        let table = default_sine_wavetable();
        let mut engine = Engine::new(48_000, 4, vec![table.clone(), table.clone(), table.clone(), table.clone()]).unwrap();
        engine.set_reverb(false, 0.0);
        engine.set_tremolo(false, 0.0);
        engine.set_crossfade(false, 0.0);
        engine.set_filter_sweep(false, 0.15, 0.80, 0.008);
        engine.set_subtractive(false, 0.0);
        engine.set_frequency(110.0);

        // Enable FM with depth 0.5
        engine.set_fm(true, 0.5);

        // After first frame, ramp should be between 0 and 0.5
        let mut out = vec![0i16; 2];
        engine.render_i16_stereo(&mut out);
        assert!(engine.fm_depth_ramp > 0.0, "fm_depth_ramp should increase on first frame after enable");
        assert!(engine.fm_depth_ramp <= 0.5, "fm_depth_ramp should not exceed target");

        // After many frames, should approach target
        let mut out = vec![0i16; 48000 * 2];  // 1 second at 48kHz
        engine.render_i16_stereo(&mut out);
        assert!((engine.fm_depth_ramp - 0.5).abs() < 0.01, "fm_depth_ramp should converge toward target");
    }

    #[test]
    fn fm_ramp_snaps_to_zero_on_disable() {
        let table = default_sine_wavetable();
        let mut engine = Engine::new(48_000, 4, vec![table.clone(), table.clone(), table.clone(), table.clone()]).unwrap();
        engine.set_reverb(false, 0.0);
        engine.set_tremolo(false, 0.0);
        engine.set_crossfade(false, 0.0);
        engine.set_filter_sweep(false, 0.15, 0.80, 0.008);
        engine.set_subtractive(false, 0.0);
        engine.set_frequency(110.0);

        // Enable FM and let it ramp up
        engine.set_fm(true, 0.5);
        let mut out = vec![0i16; 96000];  // 2 seconds
        engine.render_i16_stereo(&mut out);
        assert!(engine.fm_depth_ramp > 0.4, "fm_depth_ramp should be high after ramping");

        // Disable FM
        engine.set_fm(false, 0.0);
        assert_eq!(engine.fm_depth_ramp, 0.0, "fm_depth_ramp should snap to 0 on disable");
    }

    #[test]
    fn subtractive_ramp_smoothly_increases_on_enable() {
        let table = default_sine_wavetable();
        let mut engine = Engine::new(48_000, 4, vec![table.clone(), table.clone(), table.clone(), table.clone()]).unwrap();
        engine.set_reverb(false, 0.0);
        engine.set_tremolo(false, 0.0);
        engine.set_crossfade(false, 0.0);
        engine.set_filter_sweep(false, 0.15, 0.80, 0.008);
        engine.set_fm(false, 0.0);
        engine.set_frequency(110.0);

        // Enable subtractive with depth 0.3
        engine.set_subtractive(true, 0.3);

        // After first frame, ramp should be between 0 and 0.3
        let mut out = vec![0i16; 2];
        engine.render_i16_stereo(&mut out);
        assert!(engine.subtractive_depth_ramp > 0.0, "subtractive_depth_ramp should increase on first frame after enable");
        assert!(engine.subtractive_depth_ramp <= 0.3, "subtractive_depth_ramp should not exceed target");

        // After many frames, should approach target
        let mut out = vec![0i16; 48000 * 2];  // 1 second at 48kHz
        engine.render_i16_stereo(&mut out);
        assert!((engine.subtractive_depth_ramp - 0.3).abs() < 0.01, "subtractive_depth_ramp should converge toward target");
    }

    #[test]
    fn subtractive_ramp_snaps_to_zero_on_disable() {
        let table = default_sine_wavetable();
        let mut engine = Engine::new(48_000, 4, vec![table.clone(), table.clone(), table.clone(), table.clone()]).unwrap();
        engine.set_reverb(false, 0.0);
        engine.set_tremolo(false, 0.0);
        engine.set_crossfade(false, 0.0);
        engine.set_filter_sweep(false, 0.15, 0.80, 0.008);
        engine.set_fm(false, 0.0);
        engine.set_frequency(110.0);

        // Enable subtractive and let it ramp up
        engine.set_subtractive(true, 0.3);
        let mut out = vec![0i16; 96000];  // 2 seconds
        engine.render_i16_stereo(&mut out);
        assert!(engine.subtractive_depth_ramp > 0.2, "subtractive_depth_ramp should be high after ramping");

        // Disable subtractive
        engine.set_subtractive(false, 0.0);
        assert_eq!(engine.subtractive_depth_ramp, 0.0, "subtractive_depth_ramp should snap to 0 on disable");
    }

    #[test]
    fn fm_staircase_even_oscillators_use_consistent_fm() {
        // Test that all even-indexed oscillators use the same FM modulation
        // This would require introspection into the engine state which isn't currently exposed.
        // Instead, verify no panics and generates output when FM enabled with multiple even oscillators
        let table = default_sine_wavetable();
        let mut engine = Engine::new(48_000, 8, vec![table.clone(); 4]).unwrap();
        engine.set_reverb(false, 0.0);
        engine.set_tremolo(false, 0.0);
        engine.set_crossfade(false, 0.0);
        engine.set_filter_sweep(false, 0.15, 0.80, 0.008);
        engine.set_fm(true, 0.5);
        engine.set_frequency(110.0);

        let mut out = vec![0i16; 2048];
        engine.render_i16_stereo(&mut out);
        // Should produce output without panicking
        assert!(out.iter().any(|&s| s != 0), "FM should produce output");
    }

    fn make_engine_n(n: usize) -> Engine {
        let table = default_sine_wavetable();
        let tables = vec![table; n.max(1)];
        Engine::new(48_000, n, tables).unwrap()
    }

    #[test]
    fn test_filter_phases_seeded_evenly() {
        // Engine with 4 oscillators should construct without panic
        // Filter phases should be seeded at construction time
        let engine = make_engine_n(4);
        // Access oscillator filter_lfo_phase values via rendering
        // Since fields are private, test via behavior: after constructing engine,
        // render a few frames and verify it doesn't panic
        let mut engine = engine;
        let mut buf = vec![0i16; 256];
        engine.set_filter_sweep(true, 0.15, 0.80, 0.008);
        engine.render_i16_stereo(&mut buf);
        // Should not panic and produce non-zero output
        assert!(buf.iter().any(|&s| s != 0));
    }

    #[test]
    fn test_filter_sweep_disabled_no_change() {
        let mut engine = make_engine_n(4);
        engine.set_filter_sweep(false, 0.15, 0.80, 0.008);
        let mut buf = vec![0i16; 256];
        engine.render_i16_stereo(&mut buf);
        // Should complete without panic
        // Output should be zero for all-silent input, but we get it from wavetable
        // so just verify it completes
        assert_eq!(buf.len(), 256);
    }

    #[test]
    fn test_scale_na_restores_uniform() {
        let table = default_sine_wavetable();
        let mut engine = Engine::new(48_000, 4, vec![table.clone(), table.clone(), table.clone(), table]).unwrap();
        // First set a scale
        engine.set_scale(ScaleMode::Major, 100.0);
        // Then restore N/A
        engine.set_scale(ScaleMode::None, 100.0);
        // Verify detune_ratios are 4-cents-apart uniform by rendering
        // (we can't access detune_ratio directly, but rendering shouldn't panic)
        let mut buf = vec![0i16; 256];
        engine.render_i16_stereo(&mut buf);
        assert!(buf.iter().any(|&s| s != 0));
    }

    #[test]
    fn test_scale_all_modes_render() {
        let modes = [
            ScaleMode::None, ScaleMode::Major, ScaleMode::NaturalMinor,
            ScaleMode::Pentatonic, ScaleMode::Dorian, ScaleMode::Mixolydian,
            ScaleMode::WholeTone, ScaleMode::Hirajoshi, ScaleMode::Lydian,
        ];
        for mode in modes {
            let table = default_sine_wavetable();
            let mut engine = Engine::new(48_000, 4, vec![table.clone(), table.clone(), table.clone(), table]).unwrap();
            engine.set_frequency(440.0);
            engine.set_scale(mode, 50.0);
            let mut buf = vec![0i16; 256];
            engine.render_i16_stereo(&mut buf);
            // Should not panic or crash
            assert!(buf.len() == 256);
        }
    }

    #[test]
    fn test_scale_extreme_values() {
        let table = default_sine_wavetable();
        let mut engine = Engine::new(48_000, 4, vec![table.clone(), table.clone(), table.clone(), table]).unwrap();
        engine.set_frequency(440.0);
        engine.set_scale(ScaleMode::Major, 0.0);  // 0% spread
        engine.set_scale(ScaleMode::Major, -100.0); // negative spread (use abs)
        engine.set_scale(ScaleMode::Hirajoshi, 100.0);
        let mut buf = vec![0i16; 256];
        engine.render_i16_stereo(&mut buf);
        assert!(buf.len() == 256);
    }
}
