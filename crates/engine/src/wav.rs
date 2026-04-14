use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::types::GranularSource;

pub fn load_wav_sources(wav_dir: &Path) -> Result<Vec<GranularSource>> {
    let mut files: Vec<PathBuf> = fs::read_dir(wav_dir)
        .with_context(|| format!("failed to read WAV directory: {}", wav_dir.display()))?
        .flatten()
        .map(|e| e.path())
        .filter(|path| {
            path.is_file()
                && path
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("wav"))
        })
        .collect();
    files.sort();

    let mut out = Vec::new();
    for file in files {
        if let Some(source) = load_wav_source_file(&file)? {
            out.push(source);
        }
    }
    Ok(out)
}

fn load_wav_source_file(path: &Path) -> Result<Option<GranularSource>> {
    let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
        return Ok(None);
    };
    let bytes = fs::read(path)
        .with_context(|| format!("failed to read WAV source file: {}", path.display()))?;
    let (sample_rate, channels, bits_per_sample, audio_format, data) = parse_wav_bytes(&bytes)
        .with_context(|| format!("invalid WAV source file: {}", path.display()))?;

    let samples = decode_wav_mono_f32(data, channels, bits_per_sample, audio_format)
        .with_context(|| format!("unsupported WAV source format in {}", path.display()))?;
    if samples.len() < 2 {
        return Ok(None);
    }

    Ok(Some(GranularSource {
        name: stem.to_string(),
        sample_rate,
        samples,
    }))
}

fn parse_wav_bytes(bytes: &[u8]) -> Result<(u32, u16, u16, u16, &[u8])> {
    if bytes.len() < 44 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        anyhow::bail!("not a RIFF/WAVE file");
    }

    let mut cursor = 12usize;
    let mut sample_rate = None;
    let mut channels = None;
    let mut bits_per_sample = None;
    let mut audio_format = None;
    let mut data = None;

    while cursor + 8 <= bytes.len() {
        let chunk_id = &bytes[cursor..cursor + 4];
        let size = u32::from_le_bytes([
            bytes[cursor + 4],
            bytes[cursor + 5],
            bytes[cursor + 6],
            bytes[cursor + 7],
        ]) as usize;
        cursor += 8;
        if cursor + size > bytes.len() {
            break;
        }
        let chunk = &bytes[cursor..cursor + size];
        if chunk_id == b"fmt " && size >= 16 {
            audio_format = Some(u16::from_le_bytes([chunk[0], chunk[1]]));
            channels = Some(u16::from_le_bytes([chunk[2], chunk[3]]));
            sample_rate = Some(u32::from_le_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]));
            bits_per_sample = Some(u16::from_le_bytes([chunk[14], chunk[15]]));
        } else if chunk_id == b"data" {
            data = Some(chunk);
        }
        cursor += size + (size % 2);
    }

    Ok((
        sample_rate.context("missing fmt sample_rate")?,
        channels.context("missing fmt channels")?,
        bits_per_sample.context("missing fmt bits_per_sample")?,
        audio_format.context("missing fmt audio_format")?,
        data.context("missing data chunk")?,
    ))
}

fn decode_wav_mono_f32(
    data: &[u8],
    channels: u16,
    bits_per_sample: u16,
    audio_format: u16,
) -> Result<Vec<f32>> {
    let channels = channels.max(1) as usize;
    let frame_width_bytes = ((bits_per_sample as usize).saturating_mul(channels)) / 8;
    if frame_width_bytes == 0 {
        anyhow::bail!("invalid frame width");
    }
    let mut out = Vec::new();
    for frame in data.chunks_exact(frame_width_bytes) {
        let mut sum = 0.0f32;
        for ch in 0..channels {
            let offset = ch * (bits_per_sample as usize / 8);
            let s = match (audio_format, bits_per_sample) {
                (1, 16) => {
                    let raw = i16::from_le_bytes([frame[offset], frame[offset + 1]]);
                    raw as f32 / i16::MAX as f32
                }
                (3, 32) => f32::from_le_bytes([
                    frame[offset],
                    frame[offset + 1],
                    frame[offset + 2],
                    frame[offset + 3],
                ]),
                _ => anyhow::bail!("only PCM16 and float32 WAV sources are supported"),
            };
            sum += s;
        }
        out.push((sum / channels as f32).clamp(-1.0, 1.0));
    }
    Ok(out)
}
