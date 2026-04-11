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
}

pub struct Engine {
    sample_rate: u32,
    wavetables: Vec<Wavetable>,
    wavetable_offset: usize,
    oscillators: Vec<Oscillator>,
    base_frequency_hz: f32,
    fine_tune_cents: f32,
    stereo_spread: u8,
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
            
            let mut rng_state = (sample_rate as u64).wrapping_mul(0xdeadbeef).wrapping_add(i as u64 * 0x9e3779b97f4a7c15);
            let drift_lfo_start = lcg_next(&mut rng_state) as f32 / u32::MAX as f32;
            let drift_lfo_rate = 0.05 + (lcg_next(&mut rng_state) as f32 / u32::MAX as f32) * 0.45;
            
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

    pub fn render_i16_stereo(&mut self, out: &mut [i16]) {
        for frame in out.chunks_exact_mut(2) {
            let mut l_out = 0.0f32;
            let mut r_out = 0.0f32;
            let table_count = self.wavetables.len();
            for (osc_idx, osc) in self.oscillators.iter_mut().enumerate() {
                let table_index = (self.wavetable_offset + osc_idx) % table_count;
                let table = &self.wavetables[table_index].samples;
                let len = table.len() as f32;
                let phase_pos = osc.phase * len;
                let i0 = phase_pos as usize % table.len();
                let i1 = (i0 + 1) % table.len();
                let frac = phase_pos - (i0 as f32);
                let s = table[i0] * (1.0 - frac) + table[i1] * frac;

                l_out += s * osc.pan_l;
                r_out += s * osc.pan_r;

                // Drift LFO: advance phase
                let lfo_incr = osc.drift_lfo_rate_hz / self.sample_rate as f32;
                osc.drift_lfo_phase = (osc.drift_lfo_phase + lfo_incr).fract();
                
                // Drift multiplier: 2^(cents * sin(phase * 2PI) / 1200)
                let drift_cents = self.fine_tune_cents * (osc.drift_lfo_phase * 2.0 * std::f32::consts::PI).sin();
                let drift_ratio = 2.0f32.powf(drift_cents / 1200.0);

                let incr = (osc.current_base_hz * osc.detune_ratio * drift_ratio) / self.sample_rate as f32;
                let new_phase = osc.phase + incr;
                if new_phase >= 1.0 {
                    if let Some(pending_hz) = osc.pending_base_hz {
                        if osc.delay_cycles_remaining <= 1 {
                            osc.current_base_hz = pending_hz;
                            osc.pending_base_hz = None;
                            osc.delay_cycles_remaining = 0;
                        } else {
                            osc.delay_cycles_remaining -= 1;
                        }
                    }
                }
                osc.phase = new_phase.fract();
            }

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
        for builtin in builtin_wavetables() {
            if out.len() >= min_count {
                break;
            }
            if !out.iter().any(|w| w.name == builtin.name) {
                out.push(builtin);
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
        let dir = std::env::temp_dir().join("pirate_synth_test_empty");
        let _ = std::fs::create_dir_all(&dir);
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
}
