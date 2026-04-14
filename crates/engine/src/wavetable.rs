use std::f32::consts::PI;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::types::Wavetable;

pub(crate) fn lerp_table(table: &[f32], phase: f32) -> f32 {
    let len = table.len() as f32;
    let pos = phase * len;
    let i0 = pos as usize % table.len();
    let i1 = (i0 + 1) % table.len();
    let frac = pos - i0 as f32;
    table[i0] * (1.0 - frac) + table[i1] * frac
}

pub(crate) fn read_from_bank(tables: &[Wavetable], cur_idx: usize, next_idx: usize, phase: f32, xfade_t: f32, crossfade_enabled: bool) -> f32 {
    let s_cur = lerp_table(&tables[cur_idx].samples, phase);
    if crossfade_enabled && xfade_t > 0.0 {
        let s_next = lerp_table(&tables[next_idx].samples, phase);
        s_cur * (1.0 - xfade_t) + s_next * xfade_t
    } else {
        s_cur
    }
}

pub(crate) fn sample_from_banks(
    current: &[Wavetable],
    pending: &[Wavetable],
    bank_blend: f32,
    cur_idx: usize,
    next_idx: usize,
    phase: f32,
    xfade_t: f32,
    crossfade_enabled: bool,
) -> f32 {
    let s = read_from_bank(current, cur_idx, next_idx, phase, xfade_t, crossfade_enabled);
    if bank_blend > 0.0 && !pending.is_empty() {
        let p_len = pending.len();
        let p_cur = cur_idx % p_len;
        let p_next = (p_cur + 1) % p_len;
        // Use phase-aligned blend: same oscillator phase, both read at comparable position
        let s_p = read_from_bank(pending, p_cur, p_next, phase, xfade_t, crossfade_enabled);
        s * (1.0 - bank_blend) + s_p * bank_blend
    } else {
        s
    }
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

    if !matches!(ext, "txt" | "csv") {
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
