use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use audio_alsa::{command_channel, spawn_audio_thread, AudioCommand, AudioConfig};
use engine::{key_to_frequency_hz, load_wavetables, Engine, ScaleMode};
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
    // Scale
    #[serde(default)]
    scale_index: usize,
    // Wavetable bank (0=A, 1=B, 2=C)
    #[serde(default)]
    bank_index: usize,
    // Output volume 0-100
    #[serde(default = "default_volume")]
    volume: u8,
    // Transition duration in seconds for cents/scale/bank changes
    #[serde(default = "default_transition_secs")]
    transition_secs: f32,
    // Oscillators
    #[serde(default = "default_oscillators_active")]
    oscillators_active: bool,
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
fn default_oscillators_active() -> bool {
    false
}
fn default_transition_secs() -> f32 {
    3.0
}
fn default_volume() -> u8 {
    100
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
            scale_index: 0,
            bank_index: 0,
            volume: default_volume(),
            transition_secs: default_transition_secs(),
            oscillators_active: false,
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

/// Per-user overrides stored in `~/.pirate-synth.toml`. Every field is
/// optional so only the values you set are applied on top of the system config.
#[derive(Debug, Default, serde::Deserialize)]
struct UserConfig {
    sample_rate: Option<u32>,
    buffer_frames: Option<usize>,
    oscillators: Option<usize>,
    root_key: Option<String>,
    root_octave: Option<i32>,
    fine_tune_cents: Option<f32>,
    stereo_spread: Option<u8>,
    wavetable_dir: Option<PathBuf>,
    spi_device: Option<String>,
    reverb_enabled: Option<bool>,
    reverb_wet: Option<f32>,
    tremolo_enabled: Option<bool>,
    tremolo_depth: Option<f32>,
    crossfade_enabled: Option<bool>,
    crossfade_rate: Option<f32>,
    filter_sweep_enabled: Option<bool>,
    filter_sweep_min: Option<f32>,
    filter_sweep_max: Option<f32>,
    filter_sweep_rate_hz: Option<f32>,
    fm_enabled: Option<bool>,
    fm_depth: Option<f32>,
    subtractive_enabled: Option<bool>,
    subtractive_depth: Option<f32>,
    scale_index: Option<usize>,
    bank_index: Option<usize>,
    volume: Option<u8>,
    transition_secs: Option<f32>,
    oscillators_active: Option<bool>,
}

fn user_config_path() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".pirate-synth.toml"))
}

fn load_user_config(path: &Path) -> Result<Option<UserConfig>> {
    if !path.exists() {
        return Ok(None);
    }
    let text = fs::read_to_string(path)
        .with_context(|| format!("failed reading user config {}", path.display()))?;
    let config: UserConfig =
        toml::from_str(&text).with_context(|| format!("invalid TOML in {}", path.display()))?;
    info!("loaded user config from {}", path.display());
    Ok(Some(config))
}

fn apply_user_config(base: AppConfig, user: UserConfig) -> AppConfig {
    AppConfig {
        sample_rate: user.sample_rate.unwrap_or(base.sample_rate),
        buffer_frames: user.buffer_frames.unwrap_or(base.buffer_frames),
        oscillators: user.oscillators.unwrap_or(base.oscillators),
        root_key: user.root_key.unwrap_or(base.root_key),
        root_octave: user.root_octave.unwrap_or(base.root_octave),
        fine_tune_cents: user.fine_tune_cents.unwrap_or(base.fine_tune_cents),
        stereo_spread: user.stereo_spread.unwrap_or(base.stereo_spread),
        wavetable_dir: user.wavetable_dir.unwrap_or(base.wavetable_dir),
        spi_device: user.spi_device.unwrap_or(base.spi_device),
        reverb_enabled: user.reverb_enabled.unwrap_or(base.reverb_enabled),
        reverb_wet: user.reverb_wet.unwrap_or(base.reverb_wet),
        tremolo_enabled: user.tremolo_enabled.unwrap_or(base.tremolo_enabled),
        tremolo_depth: user.tremolo_depth.unwrap_or(base.tremolo_depth),
        crossfade_enabled: user.crossfade_enabled.unwrap_or(base.crossfade_enabled),
        crossfade_rate: user.crossfade_rate.unwrap_or(base.crossfade_rate),
        filter_sweep_enabled: user.filter_sweep_enabled.unwrap_or(base.filter_sweep_enabled),
        filter_sweep_min: user.filter_sweep_min.unwrap_or(base.filter_sweep_min),
        filter_sweep_max: user.filter_sweep_max.unwrap_or(base.filter_sweep_max),
        filter_sweep_rate_hz: user.filter_sweep_rate_hz.unwrap_or(base.filter_sweep_rate_hz),
        fm_enabled: user.fm_enabled.unwrap_or(base.fm_enabled),
        fm_depth: user.fm_depth.unwrap_or(base.fm_depth),
        subtractive_enabled: user.subtractive_enabled.unwrap_or(base.subtractive_enabled),
        subtractive_depth: user.subtractive_depth.unwrap_or(base.subtractive_depth),
        scale_index: user.scale_index.unwrap_or(base.scale_index),
        bank_index: user.bank_index.unwrap_or(base.bank_index),
        volume: user.volume.unwrap_or(base.volume),
        transition_secs: user.transition_secs.unwrap_or(base.transition_secs),
        oscillators_active: user.oscillators_active.unwrap_or(base.oscillators_active),
    }
}

fn scale_mode_from_index(idx: usize) -> ScaleMode {
    match idx {
        0 => ScaleMode::None,
        1 => ScaleMode::Major,
        2 => ScaleMode::NaturalMinor,
        3 => ScaleMode::Pentatonic,
        4 => ScaleMode::Dorian,
        5 => ScaleMode::Mixolydian,
        6 => ScaleMode::WholeTone,
        7 => ScaleMode::Hirajoshi,
        8 => ScaleMode::Lydian,
        _ => ScaleMode::None,
    }
}

fn load_bank(wavetable_dir: &Path, bank_name: &str, min_count: usize) -> Result<Vec<engine::Wavetable>> {
    let bank_dir = wavetable_dir.join(bank_name);
    if bank_dir.is_dir() {
        load_wavetables(&bank_dir, min_count)
    } else {
        // Fallback: load directly from wavetable_dir (backward compat)
        load_wavetables(wavetable_dir, min_count)
    }
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
    let config = match user_config_path() {
        Some(user_path) => match load_user_config(&user_path)? {
            Some(user) => {
                info!("applying user config from {}", user_path.display());
                apply_user_config(config, user)
            }
            None => config,
        },
        None => config,
    };
    info!(
        "audio config: sample_rate={} buffer_frames={} oscillators={} wavetable dir={} spi_device={}",
        config.sample_rate,
        config.buffer_frames,
        config.oscillators,
        config.wavetable_dir.display(),
        config.spi_device
    );

    let initial_bank = ui::BANK_NAMES
        .get(config.bank_index)
        .copied()
        .unwrap_or("A");
    let wavetables = load_bank(&config.wavetable_dir, initial_bank, config.oscillators).with_context(|| {
        format!(
            "failed loading wavetables from {}/{}",
            config.wavetable_dir.display(),
            initial_bank
        )
    })?;
    info!(
        "loaded {} wavetable(s) from {}/{}",
        wavetables.len(),
        config.wavetable_dir.display(),
        initial_bank
    );

    let mut menu = MenuState::new(config.root_octave, config.fine_tune_cents);
    menu.key_index = ui::KEY_NAMES
        .iter()
        .position(|k| *k == config.root_key)
        .unwrap_or(0);
    menu.stereo_spread = config.stereo_spread;
    menu.scale_index = config.scale_index.min(ui::SCALE_NAMES.len() - 1);
    menu.bank_index = config.bank_index.min(ui::BANK_NAMES.len() - 1);
    menu.volume = config.volume.min(100);
    menu.oscillators_active = config.oscillators_active;
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
    engine.set_scale(scale_mode_from_index(config.scale_index), config.fine_tune_cents);
    engine.set_transition_secs(config.transition_secs);
    engine.set_volume(config.volume);
    engine.set_oscillators_active_immediate(config.oscillators_active);
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

    const DEBOUNCE: Duration = Duration::from_millis(200);
    let mut pending_cents: Option<(f32, Instant)> = None;
    let mut pending_scale: Option<(usize, Instant)> = None;
    let mut pending_bank: Option<(usize, Instant)> = None;

    loop {
        if let Some(button) = buttons.poll_pressed()? {
            debug!("button press: {:?}", button);
            let old_key = menu.key_name();
            let old_octave = menu.octave;
            let old_cents = menu.fine_tune_cents;
            let old_spread = menu.stereo_spread;
            let old_scale = menu.scale_index;
            let old_bank = menu.bank_index;
            let old_volume = menu.volume;
            let old_oscs = menu.oscillators_active;

            menu.apply_button(button);
            display.draw_menu(&menu)?;

            if menu.key_name() != old_key || menu.octave != old_octave {
                let hz = key_to_frequency_hz(menu.key_name(), menu.octave, 0.0)?;
                if let Err(err) = audio_tx.try_send(AudioCommand::SetFrequencyHz(hz)) {
                    warn!("failed to send frequency update to audio thread: {err}");
                }
            }

            if menu.fine_tune_cents != old_cents {
                pending_cents = Some((menu.fine_tune_cents, Instant::now()));
            }

            if menu.stereo_spread != old_spread {
                if let Err(err) = audio_tx.try_send(AudioCommand::SetStereoSpread(menu.stereo_spread)) {
                    warn!("failed to send stereo spread to audio thread: {err}");
                }
            }

            if menu.scale_index != old_scale {
                pending_scale = Some((menu.scale_index, Instant::now()));
            }

            if menu.bank_index != old_bank {
                pending_bank = Some((menu.bank_index, Instant::now()));
            }

            if menu.volume != old_volume {
                if let Err(err) = audio_tx.try_send(AudioCommand::SetVolume(menu.volume)) {
                    warn!("failed to send volume to audio thread: {err}");
                }
            }

            if menu.oscillators_active != old_oscs {
                if let Err(err) = audio_tx.try_send(AudioCommand::SetOscillatorsActive(menu.oscillators_active)) {
                    warn!("failed to send oscillators active to audio thread: {err}");
                }
            }
        }

        // Flush debounced changes to audio thread
        let now = Instant::now();
        if let Some((cents, since)) = pending_cents {
            if now.duration_since(since) >= DEBOUNCE {
                if let Err(err) = audio_tx.try_send(AudioCommand::SetFineTuneCents(cents)) {
                    warn!("failed to send fine tune cents to audio thread: {err}");
                }
                // Also resend scale since spread_percent ties to cents
                if let Err(err) = audio_tx.try_send(AudioCommand::SetScale {
                    mode: scale_mode_from_index(menu.scale_index),
                    spread_percent: cents,
                }) {
                    warn!("failed to send scale update to audio thread: {err}");
                }
                pending_cents = None;
            }
        }
        if let Some((scale_idx, since)) = pending_scale {
            if now.duration_since(since) >= DEBOUNCE {
                if let Err(err) = audio_tx.try_send(AudioCommand::SetScale {
                    mode: scale_mode_from_index(scale_idx),
                    spread_percent: menu.fine_tune_cents,
                }) {
                    warn!("failed to send scale update to audio thread: {err}");
                }
                pending_scale = None;
            }
        }
        if let Some((bank_idx, since)) = pending_bank {
            if now.duration_since(since) >= DEBOUNCE {
                let bank_name = ui::BANK_NAMES.get(bank_idx).copied().unwrap_or("A");
                match load_bank(&config.wavetable_dir, bank_name, config.oscillators) {
                    Ok(tables) => {
                        if let Err(err) = audio_tx.try_send(AudioCommand::SetWavetableBank(tables)) {
                            warn!("failed to send wavetable bank to audio thread: {err}");
                        }
                    }
                    Err(err) => warn!("failed to load wavetable bank {bank_name}: {err}"),
                }
                pending_bank = None;
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
    }
    #[test]
    fn apply_user_config_overrides_selected_fields() {
        let base = AppConfig::default();
        let user = UserConfig {
            root_key: Some("G".into()),
            root_octave: Some(4),
            fm_enabled: Some(true),
            ..UserConfig::default()
        };
        let merged = apply_user_config(base, user);
        assert_eq!(merged.root_key, "G");
        assert_eq!(merged.root_octave, 4);
        assert!(merged.fm_enabled);
        // unchanged fields retain defaults
        assert_eq!(merged.sample_rate, 48_000);
        assert_eq!(merged.oscillators, 8);
        assert!(merged.reverb_enabled);
    }
}
