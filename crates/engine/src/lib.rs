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
}

pub struct Engine {
    sample_rate: u32,
    wavetables: Vec<Wavetable>,
    wavetable_offset: usize,
    oscillators: Vec<Oscillator>,
    base_frequency_hz: f32,
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

        let oscillators = (0..oscillator_count)
            .map(|i| {
                let center = (oscillator_count.saturating_sub(1) as f32) / 2.0;
                let cents = (i as f32 - center) * 4.0;
                Oscillator {
                    phase: 0.0,
                    detune_ratio: 2.0f32.powf(cents / 1200.0),
                }
            })
            .collect();

        Ok(Self {
            sample_rate,
            wavetables,
            wavetable_offset: 0,
            oscillators,
            base_frequency_hz: 65.406_39,
        })
    }

    pub fn oscillator_count(&self) -> usize {
        self.oscillators.len()
    }

    pub fn set_frequency(&mut self, frequency_hz: f32) {
        self.base_frequency_hz = frequency_hz.max(1.0);
    }

    pub fn set_wavetable_offset(&mut self, offset: usize) {
        if !self.wavetables.is_empty() {
            self.wavetable_offset = offset % self.wavetables.len();
        }
    }

    pub fn render_i16_stereo(&mut self, out: &mut [i16]) {
        for frame in out.chunks_exact_mut(2) {
            let mut sample = 0.0f32;
            let table_count = self.wavetables.len();
            for (osc_idx, osc) in self.oscillators.iter_mut().enumerate() {
                let table_index = (self.wavetable_offset + osc_idx) % table_count;
                let table = &self.wavetables[table_index].samples;
                let len = table.len() as f32;
                let phase = osc.phase * len;
                let i0 = phase as usize % table.len();
                let i1 = (i0 + 1) % table.len();
                let frac = phase - (i0 as f32);
                let s = table[i0] * (1.0 - frac) + table[i1] * frac;
                sample += s;

                let incr = (self.base_frequency_hz * osc.detune_ratio) / self.sample_rate as f32;
                osc.phase = (osc.phase + incr).fract();
            }

            let gain = 0.25f32 / self.oscillators.len() as f32;
            let s = (sample * gain).clamp(-1.0, 1.0);
            let v = (s * i16::MAX as f32) as i16;
            frame[0] = v;
            frame[1] = v;
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

pub fn load_wavetables(wavetable_dir: &Path) -> Result<Vec<Wavetable>> {
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

    if out.is_empty() {
        out.push(default_sine_wavetable());
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
}
