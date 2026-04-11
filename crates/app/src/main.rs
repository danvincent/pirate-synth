use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use audio_alsa::{command_channel, spawn_audio_thread, AudioCommand, AudioConfig};
use engine::{key_to_frequency_hz, load_wavetables, Engine};
use log::{debug, info, warn};
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
    #[serde(default)]
    stereo_spread: u8,
    #[serde(default = "default_wavetable_dir")]
    wavetable_dir: PathBuf,
    #[serde(default = "default_spi_device")]
    spi_device: String,
    // Reverb
    #[serde(default = "default_reverb_enabled")]
    reverb_enabled: bool,
    #[serde(default = "default_reverb_wet")]
    reverb_wet: f32,
    // Tremolo
    #[serde(default = "default_tremolo_enabled")]
    tremolo_enabled: bool,
    #[serde(default = "default_tremolo_depth")]
    tremolo_depth: f32,
    // Crossfade
    #[serde(default = "default_crossfade_enabled")]
    crossfade_enabled: bool,
    #[serde(default = "default_crossfade_rate")]
    crossfade_rate: f32,
    // Filter sweep
    #[serde(default = "default_filter_sweep_enabled")]
    filter_sweep_enabled: bool,
    #[serde(default = "default_filter_sweep_min")]
    filter_sweep_min: f32,
    #[serde(default = "default_filter_sweep_max")]
    filter_sweep_max: f32,
    #[serde(default = "default_filter_sweep_rate_hz")]
    filter_sweep_rate_hz: f32,
    // FM
    #[serde(default)]
    fm_enabled: bool,
    #[serde(default = "default_fm_depth")]
    fm_depth: f32,
    // Subtractive
    #[serde(default)]
    subtractive_enabled: bool,
    #[serde(default = "default_subtractive_depth")]
    subtractive_depth: f32,
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
    "/dev/spidev0.1".into()
}

fn default_reverb_enabled() -> bool {
    true
}
fn default_reverb_wet() -> f32 {
    0.20
}
fn default_tremolo_enabled() -> bool {
    true
}
fn default_tremolo_depth() -> f32 {
    0.35
}
fn default_crossfade_enabled() -> bool {
    true
}
fn default_crossfade_rate() -> f32 {
    0.05
}
fn default_filter_sweep_enabled() -> bool {
    true
}
fn default_filter_sweep_min() -> f32 {
    0.15
}
fn default_filter_sweep_max() -> f32 {
    0.80
}
fn default_filter_sweep_rate_hz() -> f32 {
    0.008
}
fn default_fm_depth() -> f32 {
    0.15
}
fn default_subtractive_depth() -> f32 {
    0.30
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
            stereo_spread: 0,
            wavetable_dir: default_wavetable_dir(),
            spi_device: default_spi_device(),
            reverb_enabled: default_reverb_enabled(),
            reverb_wet: default_reverb_wet(),
            tremolo_enabled: default_tremolo_enabled(),
            tremolo_depth: default_tremolo_depth(),
            crossfade_enabled: default_crossfade_enabled(),
            crossfade_rate: default_crossfade_rate(),
            filter_sweep_enabled: default_filter_sweep_enabled(),
            filter_sweep_min: default_filter_sweep_min(),
            filter_sweep_max: default_filter_sweep_max(),
            filter_sweep_rate_hz: default_filter_sweep_rate_hz(),
            fm_enabled: false,
            fm_depth: default_fm_depth(),
            subtractive_enabled: false,
            subtractive_depth: default_subtractive_depth(),
        }
    }
}

fn load_config(path: &Path) -> Result<AppConfig> {
    if !path.exists() {
        warn!(
            "config file {} missing, using built-in defaults",
            path.display()
        );
        return Ok(AppConfig::default());
    }
    let text = fs::read_to_string(path)
        .with_context(|| format!("failed reading config {}", path.display()))?;
    let config =
        toml::from_str(&text).with_context(|| format!("invalid TOML in {}", path.display()))?;
    info!("loaded config from {}", path.display());
    Ok(config)
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info,pirate_synth=info,ui=info"),
    )
    .format_timestamp_secs()
    .init();

    let args: Vec<String> = std::env::args().collect();
    info!("pirate_synth starting");

    let config_path = std::env::var("PIRATE_SYNTH_CONFIG")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(DEFAULT_CONFIG_PATH));
    info!("using config path {}", config_path.display());
    let config = load_config(&config_path)?;
    info!(
        "audio config: sample_rate={} buffer_frames={} oscillators={} wavetable dir={} spi_device={}",
        config.sample_rate,
        config.buffer_frames,
        config.oscillators,
        config.wavetable_dir.display(),
        config.spi_device
    );

    let wavetables = load_wavetables(&config.wavetable_dir, config.oscillators).with_context(|| {
        format!(
            "failed loading wavetables from {}",
            config.wavetable_dir.display()
        )
    })?;
    info!(
        "loaded {} wavetable(s) from {}",
        wavetables.len(),
        config.wavetable_dir.display()
    );

    let mut menu = MenuState::new(config.root_octave, config.fine_tune_cents);
    menu.key_index = ui::KEY_NAMES
        .iter()
        .position(|k| *k == config.root_key)
        .unwrap_or(0);
    menu.stereo_spread = config.stereo_spread;
    if menu.key_name() != config.root_key {
        warn!(
            "unknown root_key '{}' in config, falling back to '{}'",
            config.root_key,
            menu.key_name()
        );
    }

    if args.iter().any(|arg| arg == "--render-ui") {
        let out = PathBuf::from("/tmp/pirate-synth-menu.ppm");
        St7789Display::draw_menu_to_ppm(&menu, &out)?;
        info!("rendered UI preview to {}", out.display());
        return Ok(());
    }

    info!("initializing synth engine");
    let mut engine = Engine::new(config.sample_rate, config.oscillators, wavetables.clone())?;
    let initial_hz = key_to_frequency_hz(menu.key_name(), menu.octave, 0.0)?;
    engine.set_frequency(initial_hz);
    engine.set_fine_tune_cents(menu.fine_tune_cents);
    engine.set_stereo_spread(menu.stereo_spread);
    engine.set_reverb(config.reverb_enabled, config.reverb_wet);
    engine.set_tremolo(config.tremolo_enabled, config.tremolo_depth);
    engine.set_crossfade(config.crossfade_enabled, config.crossfade_rate);
    engine.set_filter_sweep(config.filter_sweep_enabled, config.filter_sweep_min, config.filter_sweep_max, config.filter_sweep_rate_hz);
    engine.set_fm(config.fm_enabled, config.fm_depth);
    engine.set_subtractive(config.subtractive_enabled, config.subtractive_depth);
    info!("initial frequency set to {:.2} Hz", initial_hz);

    let (audio_tx, audio_rx) = command_channel();
    info!("starting ALSA audio thread");
    let audio_handle = spawn_audio_thread(
        engine,
        AudioConfig {
            sample_rate: config.sample_rate,
            buffer_frames: config.buffer_frames,
        },
        audio_rx,
    );

    info!("initializing button GPIO inputs");
    let mut buttons = ButtonReader::new().context("failed to configure Pirate Audio buttons")?;
    info!("button GPIO inputs initialized");

    info!("initializing ST7789 display over {}", config.spi_device);
    let mut display = St7789Display::new(&config.spi_device, 9, Some(13))
        .context("failed to initialize ST7789 display")?;
    info!("display initialized (DC=BCM9, backlight=BCM13)");

    info!("rendering initial menu frame");
    display.draw_menu(&menu)?;
    info!("startup complete");

    loop {
        if let Some(button) = buttons.poll_pressed()? {
            debug!("button press: {:?}", button);
            let old_key = menu.key_name();
            let old_octave = menu.octave;
            let old_cents = menu.fine_tune_cents;
            let old_spread = menu.stereo_spread;

            menu.apply_button(button);
            display.draw_menu(&menu)?;

            if menu.key_name() != old_key || menu.octave != old_octave {
                let hz = key_to_frequency_hz(menu.key_name(), menu.octave, 0.0)?;
                if let Err(err) = audio_tx.try_send(AudioCommand::SetFrequencyHz(hz)) {
                    warn!("failed to send frequency update to audio thread: {err}");
                }
            }

            if menu.fine_tune_cents != old_cents {
                if let Err(err) = audio_tx.try_send(AudioCommand::SetFineTuneCents(menu.fine_tune_cents)) {
                    warn!("failed to send fine tune cents to audio thread: {err}");
                }
            }

            if menu.stereo_spread != old_spread {
                if let Err(err) = audio_tx.try_send(AudioCommand::SetStereoSpread(menu.stereo_spread)) {
                    warn!("failed to send stereo spread to audio thread: {err}");
                }
            }
        }

        std::thread::sleep(Duration::from_millis(25));

        if args.iter().any(|arg| arg == "--oneshot") {
            info!("--oneshot enabled, requesting audio stop");
            if let Err(err) = audio_tx.send(AudioCommand::Stop) {
                warn!("failed to send stop command to audio thread: {err}");
            }
            break;
        }
    }

    info!("waiting for audio thread to exit");
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

    #[test]
    fn load_config_defaults_feature_flags() {
        let path = PathBuf::from("/tmp/does-not-exist-pirate-synth.toml");
        let config = load_config(&path).unwrap();
        assert!(config.reverb_enabled);
        assert!((config.reverb_wet - 0.20).abs() < 0.001);
        assert!(config.tremolo_enabled);
        assert!((config.tremolo_depth - 0.35).abs() < 0.001);
        assert!(config.crossfade_enabled);
        assert!(!config.fm_enabled);
        assert!(!config.subtractive_enabled);
    }
}
