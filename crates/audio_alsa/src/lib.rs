use std::io::Write;
use std::process::{Command, Stdio};
use std::thread;

use anyhow::{Context, Result};
use crossbeam_channel::{Receiver, Sender};
use engine::{Engine, ScaleMode, Wavetable};

#[derive(Clone, Debug)]
pub struct AudioConfig {
    pub sample_rate: u32,
    pub buffer_frames: usize,
}

#[derive(Clone, Debug)]
pub enum AudioCommand {
    SetFrequencyHz(f32),
    SetWavetableOffset(usize),
    SetFineTuneCents(f32),
    SetStereoSpread(u8),
    SetReverb {
        enabled: bool,
        wet: f32,
    },
    SetTremolo {
        enabled: bool,
        depth: f32,
    },
    SetCrossfade {
        enabled: bool,
        rate: f32,
    },
    SetFilterSweep {
        enabled: bool,
        min: f32,
        max: f32,
        rate_hz: f32,
    },
    SetFm {
        enabled: bool,
        depth: f32,
    },
    SetSubtractive {
        enabled: bool,
        depth: f32,
    },
    SetScale {
        mode: ScaleMode,
        spread_percent: f32,
    },
    SetWavetableBank(Vec<Wavetable>),
    SetTransitionSecs(f32),
    SetVolume(u8),
    SetOscillatorsActive(bool),
    SetGranularWavs(usize),
    Stop,
}

pub fn spawn_audio_thread(
    mut engine: Engine,
    config: AudioConfig,
    command_rx: Receiver<AudioCommand>,
) -> thread::JoinHandle<Result<()>> {
    thread::spawn(move || run_audio_loop(&mut engine, &config, command_rx))
}

fn run_audio_loop(
    engine: &mut Engine,
    config: &AudioConfig,
    command_rx: Receiver<AudioCommand>,
) -> Result<()> {
    let mut child = Command::new("aplay")
        .args([
            "-q",
            "-f",
            "S16_LE",
            "-c",
            "2",
            "-r",
            &config.sample_rate.to_string(),
            "-",
        ])
        .stdin(Stdio::piped())
        .spawn()
        .context("failed to start aplay (alsa-utils)")?;

    let mut stdin = child.stdin.take().context("failed to open aplay stdin")?;

    let mut buffer = vec![0i16; config.buffer_frames * 2];
    let mut bytes = vec![0u8; config.buffer_frames * 4];

    loop {
        while let Ok(command) = command_rx.try_recv() {
            match command {
                AudioCommand::SetFrequencyHz(freq) => engine.set_frequency_scheduled(freq),
                AudioCommand::SetWavetableOffset(offset) => engine.set_wavetable_offset(offset),
                AudioCommand::SetFineTuneCents(cents) => engine.set_fine_tune_cents(cents),
                AudioCommand::SetStereoSpread(spread) => engine.set_stereo_spread(spread),
                AudioCommand::SetReverb { enabled, wet } => engine.set_reverb(enabled, wet),
                AudioCommand::SetTremolo { enabled, depth } => engine.set_tremolo(enabled, depth),
                AudioCommand::SetCrossfade { enabled, rate } => engine.set_crossfade(enabled, rate),
                AudioCommand::SetFilterSweep {
                    enabled,
                    min,
                    max,
                    rate_hz,
                } => engine.set_filter_sweep(enabled, min, max, rate_hz),
                AudioCommand::SetFm { enabled, depth } => engine.set_fm(enabled, depth),
                AudioCommand::SetSubtractive { enabled, depth } => {
                    engine.set_subtractive(enabled, depth)
                }
                AudioCommand::SetScale {
                    mode,
                    spread_percent,
                } => engine.set_scale(mode, spread_percent),
                AudioCommand::SetWavetableBank(tables) => engine.set_wavetable_bank(tables),
                AudioCommand::SetTransitionSecs(secs) => engine.set_transition_secs(secs),
                AudioCommand::SetVolume(level) => engine.set_volume(level),
                AudioCommand::SetOscillatorsActive(active) => engine.set_oscillators_active(active),
                AudioCommand::SetGranularWavs(count) => engine.set_granular_wavs(count),
                AudioCommand::Stop => {
                    drop(stdin);
                    let _ = child.wait();
                    return Ok(());
                }
            }
        }

        engine.render_i16_stereo(&mut buffer);
        i16_to_le_bytes(&buffer, &mut bytes);
        stdin
            .write_all(&bytes)
            .context("failed streaming PCM to aplay")?;
    }
}

fn i16_to_le_bytes(samples: &[i16], out: &mut [u8]) {
    for (index, sample) in samples.iter().enumerate() {
        let bytes = sample.to_le_bytes();
        let offset = index * 2;
        out[offset] = bytes[0];
        out[offset + 1] = bytes[1];
    }
}

pub fn command_channel() -> (Sender<AudioCommand>, Receiver<AudioCommand>) {
    crossbeam_channel::bounded(32)
}
