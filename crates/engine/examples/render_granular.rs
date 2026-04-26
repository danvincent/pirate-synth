use std::io::Write;
use std::path::PathBuf;

use anyhow::{anyhow, Result};
use engine::{Engine, GranularConfig, GranularSource, ScaleMode};

struct Args {
    wav_dir: Option<PathBuf>,
    out: PathBuf,
    duration_s: f32,
    density_hz: f32,
    grain_size_ms: f32,
    note_ms: f32,
    attack_ms: f32,
    release_ms: f32,
    position: f32,
    position_jitter: f32,
    max_overlap: usize,
    channels: usize,
    pitch_cents: f32,
    scale: String,
    spawn_jitter: f32,
    sample_rate: u32,
}

fn print_usage(program: &str) {
    eprintln!(
        "Usage: {program} [--wav-dir DIR] [--out PATH] [--duration-s SECONDS] [--density-hz HZ] \
         [--grain-size-ms MS] [--note-ms MS] [--attack-ms MS] [--release-ms MS] \
         [--position VALUE] [--position-jitter VALUE] [--max-overlap COUNT] [--channels COUNT] \
         [--pitch-cents CENTS] [--scale NAME] [--spawn-jitter VALUE] [--sample-rate HZ]"
    );
}

fn parse_f32_arg(key: &str, value: &str) -> Result<f32> {
    value
        .parse::<f32>()
        .map_err(|err| anyhow!("invalid value for {key}: {value} ({err})"))
}

fn parse_u32_arg(key: &str, value: &str) -> Result<u32> {
    value
        .parse::<u32>()
        .map_err(|err| anyhow!("invalid value for {key}: {value} ({err})"))
}

fn parse_usize_arg(key: &str, value: &str) -> Result<usize> {
    value
        .parse::<usize>()
        .map_err(|err| anyhow!("invalid value for {key}: {value} ({err})"))
}

fn parse_args() -> Result<Args> {
    let mut raw = std::env::args();
    let program = raw
        .next()
        .unwrap_or_else(|| "render_granular".to_owned());
    let mut iter = raw;

    let mut args = Args {
        wav_dir: None,
        out: PathBuf::from("out.wav"),
        duration_s: 10.0,
        density_hz: 4.0,
        grain_size_ms: 250.0,
        note_ms: 4000.0,
        attack_ms: 500.0,
        release_ms: 500.0,
        position: 0.5,
        position_jitter: 0.15,
        max_overlap: 16,
        channels: 4,
        pitch_cents: 1200.0,
        scale: String::from("none"),
        spawn_jitter: 0.5,
        sample_rate: 48_000,
    };

    while let Some(arg) = iter.next() {
        if arg == "--help" || arg == "-h" {
            print_usage(&program);
            std::process::exit(0);
        }

        let Some(value) = iter.next() else {
            return Err(anyhow!("missing value for {arg}"));
        };

        match arg.as_str() {
            "--wav-dir" => args.wav_dir = Some(PathBuf::from(value)),
            "--out" => args.out = PathBuf::from(value),
            "--duration-s" => args.duration_s = parse_f32_arg("--duration-s", &value)?,
            "--density-hz" => args.density_hz = parse_f32_arg("--density-hz", &value)?,
            "--grain-size-ms" => args.grain_size_ms = parse_f32_arg("--grain-size-ms", &value)?,
            "--note-ms" => args.note_ms = parse_f32_arg("--note-ms", &value)?,
            "--attack-ms" => args.attack_ms = parse_f32_arg("--attack-ms", &value)?,
            "--release-ms" => args.release_ms = parse_f32_arg("--release-ms", &value)?,
            "--position" => args.position = parse_f32_arg("--position", &value)?,
            "--position-jitter" => {
                args.position_jitter = parse_f32_arg("--position-jitter", &value)?
            }
            "--max-overlap" => args.max_overlap = parse_usize_arg("--max-overlap", &value)?,
            "--channels" => args.channels = parse_usize_arg("--channels", &value)?,
            "--pitch-cents" => args.pitch_cents = parse_f32_arg("--pitch-cents", &value)?,
            "--scale" => args.scale = value,
            "--spawn-jitter" => args.spawn_jitter = parse_f32_arg("--spawn-jitter", &value)?,
            "--sample-rate" => args.sample_rate = parse_u32_arg("--sample-rate", &value)?,
            _ => {
                eprintln!("unknown arg: {arg}");
                print_usage(&program);
                std::process::exit(1);
            }
        }
    }

    Ok(args)
}

fn parse_scale(s: &str) -> Result<ScaleMode> {
    let normalized = s.to_ascii_lowercase();
    match normalized.as_str() {
        "none" => Ok(ScaleMode::None),
        "major" => Ok(ScaleMode::Major),
        "natural_minor" | "naturalminor" => Ok(ScaleMode::NaturalMinor),
        "pentatonic" => Ok(ScaleMode::Pentatonic),
        "dorian" => Ok(ScaleMode::Dorian),
        "mixolydian" => Ok(ScaleMode::Mixolydian),
        "whole_tone" | "wholetone" => Ok(ScaleMode::WholeTone),
        "hirajoshi" => Ok(ScaleMode::Hirajoshi),
        "lydian" => Ok(ScaleMode::Lydian),
        _ => Err(anyhow!(
            "unknown scale '{s}'. Valid values: none, major, natural_minor, pentatonic, dorian, mixolydian, whole_tone, hirajoshi, lydian"
        )),
    }
}

fn synthesize_source(sample_rate: u32, freq_hz: f32, duration_s: f32) -> GranularSource {
    let sample_count = (sample_rate as f32 * duration_s.max(0.0)) as usize;
    let phase_step = std::f32::consts::TAU * freq_hz / sample_rate as f32;
    let samples = (0..sample_count)
        .map(|index| (phase_step * index as f32).sin())
        .collect();

    GranularSource {
        name: String::from("sine"),
        sample_rate,
        samples,
    }
}

fn write_wav_header(
    writer: &mut impl Write,
    sample_rate: u32,
    num_frames: usize,
) -> std::io::Result<()> {
    let frame_bytes = 4u32;
    let data_chunk_size = u32::try_from(num_frames)
        .ok()
        .and_then(|frames| frames.checked_mul(frame_bytes))
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "frame count too large"))?;
    let riff_chunk_size = 36u32
        .checked_add(data_chunk_size)
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "wav size too large"))?;
    let byte_rate = sample_rate
        .checked_mul(frame_bytes)
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "sample rate too large"))?;
    let block_align = 4u16;

    writer.write_all(b"RIFF")?;
    writer.write_all(&riff_chunk_size.to_le_bytes())?;
    writer.write_all(b"WAVE")?;
    writer.write_all(b"fmt ")?;
    writer.write_all(&16u32.to_le_bytes())?;
    writer.write_all(&1u16.to_le_bytes())?;
    writer.write_all(&2u16.to_le_bytes())?;
    writer.write_all(&sample_rate.to_le_bytes())?;
    writer.write_all(&byte_rate.to_le_bytes())?;
    writer.write_all(&block_align.to_le_bytes())?;
    writer.write_all(&16u16.to_le_bytes())?;
    writer.write_all(b"data")?;
    writer.write_all(&data_chunk_size.to_le_bytes())?;

    Ok(())
}

fn main() -> Result<()> {
    let args = parse_args()?;

    let sources = if let Some(ref dir) = args.wav_dir {
        let loaded = engine::load_wav_sources(dir)?;
        if loaded.is_empty() {
            vec![synthesize_source(args.sample_rate, 220.0, 4.0)]
        } else {
            loaded
        }
    } else {
        vec![synthesize_source(args.sample_rate, 220.0, 4.0)]
    };

    let config = GranularConfig {
        grain_size_ms: args.grain_size_ms,
        grain_note_ms: args.note_ms,
        spawn_jitter: args.spawn_jitter,
        grain_density_hz: args.density_hz,
        max_overlapping_grains: args.max_overlap,
        position: args.position,
        position_jitter: args.position_jitter,
        envelope_attack_ms: args.attack_ms,
        envelope_release_ms: args.release_ms,
        scale_mode: parse_scale(&args.scale)?,
        granular_channels: args.channels,
        granular_pitch_cents: args.pitch_cents,
    };

    let mut eng = Engine::new_granular(args.sample_rate, 1, sources, config)?;
    eng.set_granular_active_immediate(true);
    eng.set_oscillators_active_immediate(false);

    let total_frames = (args.sample_rate as f32 * args.duration_s) as usize;
    const CHUNK_FRAMES: usize = 512;
    let mut buf = vec![0i16; CHUNK_FRAMES * 2];
    let mut all_samples: Vec<i16> = Vec::with_capacity(total_frames * 2);

    let mut remaining = total_frames;
    while remaining > 0 {
        let frames = remaining.min(CHUNK_FRAMES);
        let chunk = &mut buf[..frames * 2];
        eng.render_i16_stereo(chunk);
        all_samples.extend_from_slice(chunk);
        remaining -= frames;
    }

    let file = std::fs::File::create(&args.out)?;
    let mut writer = std::io::BufWriter::new(file);
    write_wav_header(&mut writer, args.sample_rate, total_frames)?;
    for sample in &all_samples {
        writer.write_all(&sample.to_le_bytes())?;
    }
    writer.flush()?;

    println!(
        "Wrote {} frames ({:.1}s) to {}",
        total_frames,
        args.duration_s,
        args.out.display()
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{parse_scale, synthesize_source, write_wav_header};
    use engine::ScaleMode;

    #[test]
    fn parse_scale_should_return_major_when_given_major_string() {
        assert_eq!(parse_scale("major").unwrap(), ScaleMode::Major);
    }

    #[test]
    fn parse_scale_should_return_none_mode_when_given_none_string() {
        assert_eq!(parse_scale("none").unwrap(), ScaleMode::None);
    }

    #[test]
    fn parse_scale_should_be_case_insensitive_when_uppercase_given() {
        assert_eq!(parse_scale("MAJOR").unwrap(), ScaleMode::Major);
    }

    #[test]
    fn parse_scale_should_error_when_unknown_scale_given() {
        assert!(parse_scale("junk").is_err());
    }

    #[test]
    fn synthesize_source_should_produce_nonzero_samples_when_nonzero_frequency() {
        assert!(synthesize_source(48_000, 220.0, 0.1)
            .samples
            .iter()
            .any(|&sample| sample.abs() > f32::EPSILON));
    }

    #[test]
    fn synthesize_source_should_have_correct_sample_count_when_given_duration() {
        assert_eq!(synthesize_source(48_000, 440.0, 1.0).samples.len(), 48_000);
    }

    #[test]
    fn write_wav_header_should_write_riff_marker_when_called() {
        let mut bytes = Vec::new();
        write_wav_header(&mut bytes, 48_000, 100).unwrap();

        assert_eq!(&bytes[0..4], b"RIFF");
    }

    #[test]
    fn write_wav_header_should_write_wave_marker_when_called() {
        let mut bytes = Vec::new();
        write_wav_header(&mut bytes, 48_000, 100).unwrap();

        assert_eq!(&bytes[8..12], b"WAVE");
    }

    #[test]
    fn write_wav_header_should_have_correct_data_chunk_size_when_given_frames() {
        let mut bytes = Vec::new();
        write_wav_header(&mut bytes, 48_000, 100).unwrap();

        assert_eq!(u32::from_le_bytes(bytes[40..44].try_into().unwrap()), 400);
    }

    #[test]
    fn write_wav_header_should_have_correct_byte_rate_when_given_sample_rate() {
        let mut bytes = Vec::new();
        write_wav_header(&mut bytes, 48_000, 100).unwrap();

        assert_eq!(u32::from_le_bytes(bytes[28..32].try_into().unwrap()), 192_000);
    }

    #[test]
    fn write_wav_header_should_have_correct_block_align_when_stereo_16bit() {
        let mut bytes = Vec::new();
        write_wav_header(&mut bytes, 48_000, 100).unwrap();

        assert_eq!(u16::from_le_bytes(bytes[32..34].try_into().unwrap()), 4);
    }

    #[test]
    fn write_wav_header_should_have_correct_bits_per_sample_when_16bit() {
        let mut bytes = Vec::new();
        write_wav_header(&mut bytes, 48_000, 100).unwrap();

        assert_eq!(u16::from_le_bytes(bytes[34..36].try_into().unwrap()), 16);
    }
}