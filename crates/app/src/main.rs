use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use audio_alsa::{command_channel, spawn_audio_thread, AudioCommand, AudioConfig};
use engine::{key_to_frequency_hz, load_wavetables, Engine};
use serde::Deserialize;
use ui::{ButtonReader, MenuState, St7789Display};

const DEFAULT_CONFIG_PATH: &str = "/etc/pirate-synth/config.toml";

#[derive(Debug, Clone, Deserialize)]
struct AppConfig {
    #[serde(default = "default_sample_rate")]
    sample_rate: u32,
    #[serde(default = "default_buffer_frames")]
    buffer_frames: usize,
    #[serde(default = "default_oscillators")]
    oscillators: usize,
    #[serde(default = "default_root_key")]
    root_key: String,
    #[serde(default = "default_root_octave")]
    root_octave: i32,
    #[serde(default)]
    fine_tune_cents: f32,
    #[serde(default = "default_wavetable_dir")]
    wavetable_dir: PathBuf,
    #[serde(default = "default_spi_device")]
    spi_device: String,
}

fn default_sample_rate() -> u32 {
    48_000
}
fn default_buffer_frames() -> usize {
    256
}
fn default_oscillators() -> usize {
    8
}
fn default_root_key() -> String {
    "C".into()
}
fn default_root_octave() -> i32 {
    2
}
fn default_wavetable_dir() -> PathBuf {
    PathBuf::from("/var/lib/pirate-synth/wavetables")
}
fn default_spi_device() -> String {
    "/dev/spidev0.0".into()
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            sample_rate: default_sample_rate(),
            buffer_frames: default_buffer_frames(),
            oscillators: default_oscillators(),
            root_key: default_root_key(),
            root_octave: default_root_octave(),
            fine_tune_cents: 0.0,
            wavetable_dir: default_wavetable_dir(),
            spi_device: default_spi_device(),
        }
    }
}

fn load_config(path: &Path) -> Result<AppConfig> {
    if !path.exists() {
        return Ok(AppConfig::default());
    }
    let text = fs::read_to_string(path)
        .with_context(|| format!("failed reading config {}", path.display()))?;
    Ok(toml::from_str(&text).with_context(|| format!("invalid TOML in {}", path.display()))?)
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let config_path = std::env::var("PIRATE_SYNTH_CONFIG")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(DEFAULT_CONFIG_PATH));
    let config = load_config(&config_path)?;

    let wavetables = load_wavetables(&config.wavetable_dir).with_context(|| {
        format!(
            "failed loading wavetables from {}",
            config.wavetable_dir.display()
        )
    })?;
    let wavetable_names = wavetables
        .iter()
        .map(|w| w.name.clone())
        .collect::<Vec<_>>();

    let mut menu =
        MenuState::with_wavetables(wavetable_names, config.root_octave, config.fine_tune_cents);
    menu.key_index = ui::KEY_NAMES
        .iter()
        .position(|k| *k == config.root_key)
        .unwrap_or(0);

    if args.iter().any(|arg| arg == "--render-ui") {
        let out = PathBuf::from("/tmp/pirate-synth-menu.ppm");
        St7789Display::draw_menu_to_ppm(&menu, &out)?;
        println!("Rendered UI preview to {}", out.display());
        return Ok(());
    }

    let mut engine = Engine::new(config.sample_rate, config.oscillators, wavetables.clone())?;
    let initial_hz = key_to_frequency_hz(menu.key_name(), menu.octave, menu.fine_tune_cents)?;
    engine.set_frequency(initial_hz);

    let (audio_tx, audio_rx) = command_channel();
    let audio_handle = spawn_audio_thread(
        engine,
        AudioConfig {
            sample_rate: config.sample_rate,
            buffer_frames: config.buffer_frames,
        },
        audio_rx,
    );

    let mut buttons = ButtonReader::new().context("failed to configure Pirate Audio buttons")?;
    let mut display = St7789Display::new(&config.spi_device, 9, Some(13))
        .context("failed to initialize ST7789 display")?;

    display.draw_menu(&menu)?;

    loop {
        if let Some(button) = buttons.poll_pressed()? {
            let old_wavetable = menu.selected_wavetable;
            let old_key = menu.key_name();
            let old_octave = menu.octave;
            let old_cents = menu.fine_tune_cents;

            menu.apply_button(button);
            display.draw_menu(&menu)?;

            if menu.selected_wavetable != old_wavetable {
                let _ = audio_tx.send(AudioCommand::SetWavetableOffset(menu.selected_wavetable));
            }

            if menu.key_name() != old_key
                || menu.octave != old_octave
                || menu.fine_tune_cents != old_cents
            {
                let hz = key_to_frequency_hz(menu.key_name(), menu.octave, menu.fine_tune_cents)?;
                let _ = audio_tx.send(AudioCommand::SetFrequencyHz(hz));
            }
        }

        std::thread::sleep(Duration::from_millis(25));

        if args.iter().any(|arg| arg == "--oneshot") {
            let _ = audio_tx.send(AudioCommand::Stop);
            break;
        }
    }

    match audio_handle.join() {
        Ok(result) => result,
        Err(_) => Err(anyhow::anyhow!("audio thread panicked")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_config_defaults_when_missing() {
        let path = PathBuf::from("/tmp/does-not-exist-pirate-synth.toml");
        let config = load_config(&path).unwrap();
        assert_eq!(config.sample_rate, 48_000);
        assert_eq!(config.oscillators, 8);
        assert_eq!(config.root_key, "C");
    }
}
