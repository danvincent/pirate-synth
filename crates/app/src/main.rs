use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use audio_alsa::{command_channel, spawn_audio_thread, AudioCommand, AudioConfig};
use controller::SynthController;
use crossbeam_channel::{bounded, Receiver, Sender};
use engine::{
    key_to_frequency_hz, load_wav_sources, load_wavetables, Engine, GranularConfig, ScaleMode,
};
use log::{debug, info, warn};
use midir::{Ignore, MidiInput, MidiInputConnection};
use serde::Deserialize;
use ui::{
    ButtonConfig, ButtonReader, Ili9341Display, JoystickButtonReader, LinuxFbDisplay,
    MenuState, St7789Display, VideoStatus,
};
use visuals_drm::{try_spawn_visuals, VisualsInitError};

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
    #[serde(default = "default_midi_cents_cc")]
    midi_cents_cc: u8,
    #[serde(default = "default_stereo_spread")]
    stereo_spread: u8,
    #[serde(default = "default_wavetable_dir")]
    wavetable_dir: PathBuf,
    #[serde(default = "default_wav_dir")]
    wav_dir: PathBuf,
    #[serde(default)]
    spi_device: Option<String>,
    #[serde(default = "default_hardware_profile")]
    hardware_profile: String,
    #[serde(default)]
    dc_pin_override: Option<u8>,
    #[serde(default)]
    backlight_pin_override: Option<u8>,
    // Reverb
    #[serde(default = "default_reverb_enabled")]
    reverb_enabled: bool,
    #[serde(default = "default_reverb_wet")]
    reverb_wet: f32,
    #[serde(default = "default_reverb_feedback")]
    reverb_feedback: f32,
    #[serde(default = "default_reverb_damp")]
    reverb_damp: f32,
    #[serde(default = "default_reverb_comb_count")]
    reverb_comb_count: usize,
    // Granular reverb (independent)
    #[serde(default = "default_granular_reverb_enabled")]
    granular_reverb_enabled: bool,
    #[serde(default = "default_granular_reverb_wet")]
    granular_reverb_wet: f32,
    #[serde(default = "default_granular_reverb_feedback")]
    granular_reverb_feedback: f32,
    #[serde(default = "default_granular_reverb_damp")]
    granular_reverb_damp: f32,
    #[serde(default = "default_granular_reverb_comb_count")]
    granular_reverb_comb_count: usize,
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
    #[serde(default = "default_scale_index")]
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
    /// Glide duration in milliseconds for note/scale/cents changes. 0 = snap.
    #[serde(default)]
    note_transition_ms: f32,
    // Oscillators
    #[serde(default = "default_oscillators_active")]
    oscillators_active: bool,
    // Granular
    #[serde(default = "default_granular_grain_size_ms")]
    granular_grain_size_ms: f32,
    #[serde(default = "default_granular_density_hz")]
    granular_density_hz: f32,
    #[serde(default = "default_granular_max_overlap")]
    granular_max_overlap: usize,
    #[serde(default = "default_granular_position")]
    granular_position: f32,
    #[serde(default = "default_granular_position_jitter")]
    granular_position_jitter: f32,
    #[serde(default = "default_granular_attack_ms")]
    granular_attack_ms: f32,
    #[serde(default = "default_granular_release_ms")]
    granular_release_ms: f32,
    #[serde(default = "default_granular_note_ms")]
    granular_note_ms: f32,
    #[serde(default = "default_granular_spawn_jitter")]
    granular_spawn_jitter: f32,
    #[serde(default = "default_granular_channels")]
    granular_channels: usize,
    #[serde(default = "default_granular_pitch_cents")]
    granular_pitch_cents: f32,
    #[serde(default = "default_granular_wavs")]
    granular_wavs: usize,
    #[serde(default = "default_granular_volume")]
    pub granular_volume: u8,
    #[serde(default = "default_granular_active")]
    pub granular_active: bool,
    #[serde(default = "default_hdmi_visuals_enabled")]
    hdmi_visuals_enabled: bool,
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
    "A".into()
}
fn default_root_octave() -> i32 {
    1
}
fn default_midi_cents_cc() -> u8 {
    1
}
fn default_wavetable_dir() -> PathBuf {
    PathBuf::from("/var/lib/pirate-synth/wavetables")
}
fn default_wav_dir() -> PathBuf {
    PathBuf::from("/var/lib/pirate-synth/WAV")
}
fn default_hardware_profile() -> String {
    "pirate-audio".into()
}

fn default_reverb_enabled() -> bool {
    true
}
fn default_reverb_wet() -> f32 {
    0.20
}
fn default_reverb_feedback() -> f32 {
    0.84
}
fn default_reverb_damp() -> f32 {
    0.20
}
fn default_reverb_comb_count() -> usize {
    4
}
fn default_granular_reverb_enabled() -> bool {
    true
}
fn default_granular_reverb_wet() -> f32 {
    0.45
}
fn default_granular_reverb_feedback() -> f32 {
    0.88
}
fn default_granular_reverb_damp() -> f32 {
    0.12
}
fn default_granular_reverb_comb_count() -> usize {
    8
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
fn default_scale_index() -> usize {
    7
}
fn default_stereo_spread() -> u8 {
    100
}
fn default_oscillators_active() -> bool {
    false
}
fn default_transition_secs() -> f32 {
    3.0
}
fn default_volume() -> u8 {
    50
}
fn default_granular_grain_size_ms() -> f32 {
    120.0
}
fn default_granular_density_hz() -> f32 {
    24.0
}
fn default_granular_max_overlap() -> usize {
    16
}
fn default_granular_position() -> f32 {
    0.5
}
fn default_granular_position_jitter() -> f32 {
    0.15
}
fn default_granular_attack_ms() -> f32 {
    10.0
}
fn default_granular_release_ms() -> f32 {
    25.0
}
fn default_granular_note_ms() -> f32 {
    4000.0
}
fn default_granular_spawn_jitter() -> f32 {
    0.5
}
fn default_granular_channels() -> usize {
    4
}
fn default_granular_pitch_cents() -> f32 {
    1200.0
}
fn default_granular_wavs() -> usize {
    default_oscillators()
}
fn default_granular_volume() -> u8 {
    50
}
fn default_granular_active() -> bool {
    false
}
fn default_hdmi_visuals_enabled() -> bool {
    false
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
            midi_cents_cc: default_midi_cents_cc(),
            stereo_spread: default_stereo_spread(),
            wavetable_dir: default_wavetable_dir(),
            wav_dir: default_wav_dir(),
            spi_device: None,
            hardware_profile: default_hardware_profile(),
            dc_pin_override: None,
            backlight_pin_override: None,
            reverb_enabled: default_reverb_enabled(),
            reverb_wet: default_reverb_wet(),
            reverb_feedback: default_reverb_feedback(),
            reverb_damp: default_reverb_damp(),
            reverb_comb_count: default_reverb_comb_count(),
            granular_reverb_enabled: default_granular_reverb_enabled(),
            granular_reverb_wet: default_granular_reverb_wet(),
            granular_reverb_feedback: default_granular_reverb_feedback(),
            granular_reverb_damp: default_granular_reverb_damp(),
            granular_reverb_comb_count: default_granular_reverb_comb_count(),
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
            scale_index: default_scale_index(),
            bank_index: 0,
            volume: default_volume(),
            transition_secs: default_transition_secs(),
            note_transition_ms: 0.0,
            oscillators_active: false,
            granular_grain_size_ms: default_granular_grain_size_ms(),
            granular_density_hz: default_granular_density_hz(),
            granular_max_overlap: default_granular_max_overlap(),
            granular_position: default_granular_position(),
            granular_position_jitter: default_granular_position_jitter(),
            granular_attack_ms: default_granular_attack_ms(),
            granular_release_ms: default_granular_release_ms(),
            granular_note_ms: default_granular_note_ms(),
            granular_spawn_jitter: default_granular_spawn_jitter(),
            granular_channels: default_granular_channels(),
            granular_pitch_cents: default_granular_pitch_cents(),
            granular_wavs: default_granular_wavs(),
            granular_volume: default_granular_volume(),
            granular_active: default_granular_active(),
            hdmi_visuals_enabled: default_hdmi_visuals_enabled(),
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
    midi_cents_cc: Option<u8>,
    stereo_spread: Option<u8>,
    wavetable_dir: Option<PathBuf>,
    wav_dir: Option<PathBuf>,
    spi_device: Option<String>,
    hardware_profile: Option<String>,
    dc_pin_override: Option<u8>,
    backlight_pin_override: Option<u8>,
    reverb_enabled: Option<bool>,
    reverb_wet: Option<f32>,
    reverb_feedback: Option<f32>,
    reverb_damp: Option<f32>,
    reverb_comb_count: Option<usize>,
    granular_reverb_enabled: Option<bool>,
    granular_reverb_wet: Option<f32>,
    granular_reverb_feedback: Option<f32>,
    granular_reverb_damp: Option<f32>,
    granular_reverb_comb_count: Option<usize>,
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
    note_transition_ms: Option<f32>,
    oscillators_active: Option<bool>,
    granular_grain_size_ms: Option<f32>,
    granular_density_hz: Option<f32>,
    granular_max_overlap: Option<usize>,
    granular_position: Option<f32>,
    granular_position_jitter: Option<f32>,
    granular_attack_ms: Option<f32>,
    granular_release_ms: Option<f32>,
    granular_note_ms: Option<f32>,
    granular_spawn_jitter: Option<f32>,
    granular_channels: Option<usize>,
    granular_pitch_cents: Option<f32>,
    granular_wavs: Option<usize>,
    granular_volume: Option<u8>,
    granular_active: Option<bool>,
    hdmi_visuals_enabled: Option<bool>,
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
        midi_cents_cc: user.midi_cents_cc.unwrap_or(base.midi_cents_cc),
        stereo_spread: user.stereo_spread.unwrap_or(base.stereo_spread),
        wavetable_dir: user.wavetable_dir.unwrap_or(base.wavetable_dir),
        wav_dir: user.wav_dir.unwrap_or(base.wav_dir),
        spi_device: user.spi_device.or(base.spi_device),
        hardware_profile: user.hardware_profile.unwrap_or(base.hardware_profile),
        dc_pin_override: user.dc_pin_override.or(base.dc_pin_override),
        backlight_pin_override: user.backlight_pin_override.or(base.backlight_pin_override),
        reverb_enabled: user.reverb_enabled.unwrap_or(base.reverb_enabled),
        reverb_wet: user.reverb_wet.unwrap_or(base.reverb_wet),
        reverb_feedback: user.reverb_feedback.unwrap_or(base.reverb_feedback),
        reverb_damp: user.reverb_damp.unwrap_or(base.reverb_damp),
        reverb_comb_count: user.reverb_comb_count.unwrap_or(base.reverb_comb_count),
        granular_reverb_enabled: user
            .granular_reverb_enabled
            .unwrap_or(base.granular_reverb_enabled),
        granular_reverb_wet: user.granular_reverb_wet.unwrap_or(base.granular_reverb_wet),
        granular_reverb_feedback: user
            .granular_reverb_feedback
            .unwrap_or(base.granular_reverb_feedback),
        granular_reverb_damp: user
            .granular_reverb_damp
            .unwrap_or(base.granular_reverb_damp),
        granular_reverb_comb_count: user
            .granular_reverb_comb_count
            .unwrap_or(base.granular_reverb_comb_count),
        tremolo_enabled: user.tremolo_enabled.unwrap_or(base.tremolo_enabled),
        tremolo_depth: user.tremolo_depth.unwrap_or(base.tremolo_depth),
        crossfade_enabled: user.crossfade_enabled.unwrap_or(base.crossfade_enabled),
        crossfade_rate: user.crossfade_rate.unwrap_or(base.crossfade_rate),
        filter_sweep_enabled: user
            .filter_sweep_enabled
            .unwrap_or(base.filter_sweep_enabled),
        filter_sweep_min: user.filter_sweep_min.unwrap_or(base.filter_sweep_min),
        filter_sweep_max: user.filter_sweep_max.unwrap_or(base.filter_sweep_max),
        filter_sweep_rate_hz: user
            .filter_sweep_rate_hz
            .unwrap_or(base.filter_sweep_rate_hz),
        fm_enabled: user.fm_enabled.unwrap_or(base.fm_enabled),
        fm_depth: user.fm_depth.unwrap_or(base.fm_depth),
        subtractive_enabled: user.subtractive_enabled.unwrap_or(base.subtractive_enabled),
        subtractive_depth: user.subtractive_depth.unwrap_or(base.subtractive_depth),
        scale_index: user.scale_index.unwrap_or(base.scale_index),
        bank_index: user.bank_index.unwrap_or(base.bank_index),
        volume: user.volume.unwrap_or(base.volume),
        transition_secs: user.transition_secs.unwrap_or(base.transition_secs),
        note_transition_ms: user.note_transition_ms.unwrap_or(base.note_transition_ms),
        oscillators_active: user.oscillators_active.unwrap_or(base.oscillators_active),
        granular_grain_size_ms: user
            .granular_grain_size_ms
            .unwrap_or(base.granular_grain_size_ms),
        granular_density_hz: user.granular_density_hz.unwrap_or(base.granular_density_hz),
        granular_max_overlap: user
            .granular_max_overlap
            .unwrap_or(base.granular_max_overlap),
        granular_position: user.granular_position.unwrap_or(base.granular_position),
        granular_position_jitter: user
            .granular_position_jitter
            .unwrap_or(base.granular_position_jitter),
        granular_attack_ms: user.granular_attack_ms.unwrap_or(base.granular_attack_ms),
        granular_release_ms: user.granular_release_ms.unwrap_or(base.granular_release_ms),
        granular_note_ms: user.granular_note_ms.unwrap_or(base.granular_note_ms),
        granular_spawn_jitter: user
            .granular_spawn_jitter
            .unwrap_or(base.granular_spawn_jitter),
        granular_channels: user.granular_channels.unwrap_or(base.granular_channels),
        granular_pitch_cents: user
            .granular_pitch_cents
            .unwrap_or(base.granular_pitch_cents),
        granular_wavs: user.granular_wavs.unwrap_or(base.granular_wavs),
        granular_volume: user.granular_volume.unwrap_or(base.granular_volume),
        granular_active: user.granular_active.unwrap_or(base.granular_active),
        hdmi_visuals_enabled: user
            .hdmi_visuals_enabled
            .unwrap_or(base.hdmi_visuals_enabled),
    }
}

fn granular_config(config: &AppConfig) -> GranularConfig {
    GranularConfig {
        grain_size_ms: config.granular_grain_size_ms,
        grain_note_ms: config.granular_note_ms,
        spawn_jitter: config.granular_spawn_jitter,
        grain_density_hz: config.granular_density_hz,
        max_overlapping_grains: config.granular_max_overlap.max(1),
        position: config.granular_position.clamp(0.0, 1.0),
        position_jitter: config.granular_position_jitter.clamp(0.0, 1.0),
        envelope_attack_ms: config.granular_attack_ms.max(0.0),
        envelope_release_ms: config.granular_release_ms.max(0.0),
        scale_mode: scale_mode_from_index(config.scale_index),
        granular_channels: config.granular_channels.clamp(1, 64),
        granular_pitch_cents: config.granular_pitch_cents.clamp(-2400.0, 2400.0),
    }
}

fn apply_engine_params(engine: &mut Engine, menu: &MenuState, config: &AppConfig) {
    engine.set_fine_tune_cents(menu.fine_tune_cents);
    engine.set_stereo_spread(menu.stereo_spread);
    engine.set_reverb(config.reverb_enabled, config.reverb_wet);
    engine.set_reverb_feedback(
        config.reverb_feedback,
        config.reverb_damp,
        config.reverb_comb_count,
    );
    engine.set_granular_reverb(
        config.granular_reverb_enabled,
        config.granular_reverb_wet,
        config.granular_reverb_feedback,
        config.granular_reverb_damp,
        config.granular_reverb_comb_count,
    );
    engine.set_tremolo(config.tremolo_enabled, config.tremolo_depth);
    engine.set_crossfade(config.crossfade_enabled, config.crossfade_rate);
    engine.set_filter_sweep(
        config.filter_sweep_enabled,
        config.filter_sweep_min,
        config.filter_sweep_max,
        config.filter_sweep_rate_hz,
    );
    engine.set_fm(config.fm_enabled, config.fm_depth);
    engine.set_subtractive(config.subtractive_enabled, config.subtractive_depth);
    engine.set_granular_config(granular_config(config));
    engine.set_granular_wavs(menu.gr_voices);
    engine.set_granular_volume(config.granular_volume);
}

fn initialize_engine(config: &AppConfig, bank_name: &str) -> Result<Engine> {
    let wav_sources = if config.wav_dir.exists() {
        load_wav_sources(&config.wav_dir).with_context(|| {
            format!(
                "failed loading WAV granular sources from {}",
                config.wav_dir.display()
            )
        })?
    } else {
        Vec::new()
    };
    if wav_sources.is_empty() {
        let wavetables = load_bank(&config.wavetable_dir, bank_name, config.oscillators)
            .with_context(|| {
                format!(
                    "failed loading wavetables from {}/{}",
                    config.wavetable_dir.display(),
                    bank_name
                )
            })?;
        info!(
            "loaded {} wavetable(s) from {}/{}",
            wavetables.len(),
            config.wavetable_dir.display(),
            bank_name
        );
        Engine::new(config.sample_rate, config.oscillators, wavetables)
    } else {
        info!(
            "loaded {} WAV source file(s) from {} (granular mode)",
            wav_sources.len(),
            config.wav_dir.display()
        );
        Engine::new_granular(
            config.sample_rate,
            config.oscillators,
            wav_sources,
            granular_config(config),
        )
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

fn load_bank(
    wavetable_dir: &Path,
    bank_name: &str,
    min_count: usize,
) -> Result<Vec<engine::Wavetable>> {
    let bank_dir = wavetable_dir.join(bank_name);
    if bank_dir.is_dir() {
        load_wavetables(&bank_dir, min_count)
    } else {
        // Fallback: load directly from wavetable_dir (backward compat)
        load_wavetables(wavetable_dir, min_count)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HardwareProfile {
    PirateAudio,
    GpiCase,
}

impl HardwareProfile {
    fn from_str(s: &str) -> Result<Self> {
        match s {
            "pirate-audio" => Ok(Self::PirateAudio),
            "gpi-case" => Ok(Self::GpiCase),
            other => anyhow::bail!("Unknown hardware_profile: '{}'", other),
        }
    }

    fn default_spi_device(self) -> &'static str {
        match self {
            Self::PirateAudio => "/dev/spidev0.1",
            Self::GpiCase => "/dev/spidev0.0",
        }
    }

    fn default_dc_pin(self) -> u8 {
        match self {
            Self::PirateAudio => 9,
            Self::GpiCase => 25,
        }
    }

    fn default_backlight_pin(self) -> Option<u8> {
        match self {
            Self::PirateAudio => Some(13),
            Self::GpiCase => None,
        }
    }

    fn default_alsa_device(self) -> Option<&'static str> {
        match self {
            Self::PirateAudio => None,
            Self::GpiCase => Some("plughw:Headphones"),
        }
    }

    fn button_config(self) -> Result<ButtonConfig> {
        match self {
            Self::PirateAudio => Ok(ButtonConfig::pirate_audio()),
            Self::GpiCase => ButtonConfig::new(vec![], Some(26)),
        }
    }
}

fn resolve_spi_device(profile: &HardwareProfile, config: &AppConfig) -> String {
    config
        .spi_device
        .clone()
        .unwrap_or_else(|| profile.default_spi_device().to_string())
}

fn resolve_dc_pin(profile: &HardwareProfile, config: &AppConfig) -> u8 {
    config
        .dc_pin_override
        .unwrap_or_else(|| profile.default_dc_pin())
}

fn resolve_backlight_pin(profile: &HardwareProfile, config: &AppConfig) -> Option<u8> {
    config
        .backlight_pin_override
        .or_else(|| profile.default_backlight_pin())
}

enum AnyDisplay {
    St7789(St7789Display),
    #[allow(dead_code)]
    Ili9341(Ili9341Display),
    LinuxFb(LinuxFbDisplay),
}

impl AnyDisplay {
    fn draw_menu(&mut self, state: &MenuState) -> Result<()> {
        match self {
            Self::St7789(display) => display.draw_menu(state),
            Self::Ili9341(display) => display.draw_menu(state),
            Self::LinuxFb(display) => display.draw_menu(state),
        }
    }

    fn draw_idle_screen(&mut self, state: &MenuState, hostname: &str) -> Result<()> {
        match self {
            Self::St7789(display) => display.draw_idle_screen(state, hostname),
            Self::Ili9341(display) => display.draw_idle_screen(state, hostname),
            Self::LinuxFb(display) => display.draw_idle_screen(state, hostname),
        }
    }

    fn draw_powering_down_screen(&mut self) -> Result<()> {
        match self {
            Self::St7789(display) => display.draw_powering_down_screen(),
            Self::Ili9341(display) => display.draw_powering_down_screen(),
            Self::LinuxFb(display) => display.draw_powering_down_screen(),
        }
    }

    fn clear_and_backlight_off(&mut self) -> Result<()> {
        match self {
            Self::St7789(display) => display.clear_and_backlight_off(),
            Self::Ili9341(display) => display.clear_and_backlight_off(),
            Self::LinuxFb(display) => display.clear_and_backlight_off(),
        }
    }
}

struct HardwareBuild {
    profile: HardwareProfile,
    display: AnyDisplay,
    button_config: ButtonConfig,
    alsa_device: Option<String>,
    spi_device: String,
    dc_pin: u8,
    backlight_pin: Option<u8>,
    #[allow(dead_code)]
    power_latch: Option<rppal::gpio::OutputPin>,
    joystick_reader: Option<JoystickButtonReader>,
}

fn build_hardware(config: &AppConfig) -> Result<HardwareBuild> {
    let profile = HardwareProfile::from_str(&config.hardware_profile)?;
    let spi_device = resolve_spi_device(&profile, config);
    let dc_pin = resolve_dc_pin(&profile, config);
    let backlight_pin = resolve_backlight_pin(&profile, config);
    let button_config = profile.button_config()?;
    let alsa_device = profile
        .default_alsa_device()
        .map(|device| device.to_string());

    let (display, power_latch, joystick_reader) = match profile {
        HardwareProfile::PirateAudio => {
            let d = AnyDisplay::St7789(St7789Display::new(&spi_device, dc_pin, backlight_pin)?);
            (d, None, None)
        }
        HardwareProfile::GpiCase => {
            let d = AnyDisplay::LinuxFb(LinuxFbDisplay::new("/dev/fb0", 320, 240)?);
            let gpio = rppal::gpio::Gpio::new().context("failed to open GPIO")?;
            let latch = gpio
                .get(27)
                .context("failed to get BCM27 (power latch)")?
                .into_output_high();
            let joystick = JoystickButtonReader::new("/dev/input/js0")
                .context("failed to open joystick /dev/input/js0")?;
            (d, Some(latch), Some(joystick))
        }
    };

    Ok(HardwareBuild {
        profile,
        display,
        button_config,
        alsa_device,
        spi_device,
        dc_pin,
        backlight_pin,
        power_latch,
        joystick_reader,
    })
}

fn request_shutdown(display: &mut AnyDisplay, audio_tx: &Sender<AudioCommand>) -> bool {
    if let Err(err) = display.draw_powering_down_screen() {
        warn!("failed to draw powering down screen: {err}");
    }
    std::thread::sleep(Duration::from_millis(500));
    if let Err(err) = audio_tx.send_timeout(AudioCommand::Stop, Duration::from_millis(200)) {
        warn!("failed to send stop to audio thread before shutdown: {err}");
    }
    if let Err(err) = display.clear_and_backlight_off() {
        warn!("failed to clear display before shutdown: {err}");
    }

    match std::process::Command::new("/sbin/shutdown")
        .args(["-h", "now"])
        .status()
    {
        Ok(status) if status.success() => true,
        Ok(status) => {
            warn!("/sbin/shutdown exited with non-zero status: {status}");
            false
        }
        Err(err) => {
            warn!("failed to invoke /sbin/shutdown: {err}");
            false
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MidiEvent {
    NoteOn(u8),
    ControlChange { controller: u8, value: u8 },
}

struct MidiRuntime {
    rx: Receiver<MidiEvent>,
    _connection: MidiInputConnection<()>,
}

fn parse_midi_event(message: &[u8]) -> Option<MidiEvent> {
    if message.len() < 3 {
        return None;
    }
    let status = message[0] & 0xF0;
    let data1 = message[1] & 0x7F;
    let data2 = message[2] & 0x7F;
    match status {
        0x90 if data2 > 0 => Some(MidiEvent::NoteOn(data1)),
        0xB0 => Some(MidiEvent::ControlChange {
            controller: data1,
            value: data2,
        }),
        _ => None,
    }
}

fn midi_note_to_menu_key_octave(note: u8) -> (usize, i32) {
    let key_index = (note % 12) as usize;
    let octave = ((note / 12) as i32 - 1).clamp(0, 8);
    (key_index, octave)
}

fn midi_cc_to_cents(value: u8) -> f32 {
    ((value as f32 / 127.0) * 200.0) - 100.0
}

fn validate_midi_cc(controller: u8) -> Result<u8> {
    if controller <= 127 {
        Ok(controller)
    } else {
        Err(anyhow::anyhow!(
            "invalid midi_cents_cc value {controller}; expected range 0..=127"
        ))
    }
}

fn initialize_midi() -> Result<Option<MidiRuntime>> {
    let mut midi_in =
        MidiInput::new("pirate-synth-midi").context("failed to initialize MIDI input")?;
    midi_in.ignore(Ignore::Time | Ignore::ActiveSense | Ignore::Sysex);
    let ports = midi_in.ports();
    if ports.is_empty() {
        info!("no MIDI input ports detected");
        return Ok(None);
    }

    // Log all detected ports so users can debug mismatches.
    let port_names: Vec<String> = ports
        .iter()
        .map(|p| {
            midi_in
                .port_name(p)
                .unwrap_or_else(|_| "unknown".to_string())
        })
        .collect();
    info!("detected MIDI input ports: {:?}", port_names);

    // Prefer the first port that is not a virtual passthrough (e.g. "Midi Through").
    // Fall back to ports[0] if every port looks virtual.
    let selected_index = port_names
        .iter()
        .position(|name| !name.to_lowercase().contains("midi through"))
        .unwrap_or(0);

    let port = ports[selected_index].clone();
    let port_name = port_names[selected_index].clone();

    let (tx, rx) = bounded(64);
    let connection = midi_in
        .connect(
            &port,
            "pirate-synth-midi-in",
            move |_timestamp, message, _| {
                if let Some(event) = parse_midi_event(message) {
                    let _ = tx.try_send(event);
                }
            },
            (),
        )
        .map_err(|err| anyhow::anyhow!("failed to connect to MIDI input '{port_name}': {err}"))?;
    info!("connected MIDI input: {port_name}");
    Ok(Some(MidiRuntime {
        rx,
        _connection: connection,
    }))
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
    let midi_cents_cc = validate_midi_cc(config.midi_cents_cc)?;
    info!(
        "audio config: sample_rate={} buffer_frames={} oscillators={} wavetable dir={} wav dir={} hardware_profile={} spi_device={}",
        config.sample_rate,
        config.buffer_frames,
        config.oscillators,
        config.wavetable_dir.display(),
        config.wav_dir.display(),
        config.hardware_profile,
        config.spi_device.as_deref().unwrap_or("(profile default)")
    );

    let HardwareBuild {
        profile: hw_profile,
        display: hw_display,
        button_config: hw_button_config,
        alsa_device: hw_alsa_device,
        spi_device: hw_spi_device,
        dc_pin: hw_dc_pin,
        backlight_pin: hw_backlight_pin,
        power_latch: _power_latch,
        joystick_reader: mut hw_joystick_reader,
    } = build_hardware(&config)?;

    let initial_bank = ui::BANK_NAMES
        .get(config.bank_index)
        .copied()
        .unwrap_or("A");
    let mut menu = MenuState::new(
        config.fine_tune_cents,
        config.oscillators,
        config.granular_wavs,
    );
    menu.key_index = ui::KEY_NAMES
        .iter()
        .position(|k| *k == config.root_key)
        .unwrap_or(9);
    menu.octave = config.root_octave;
    menu.stereo_spread = config.stereo_spread;
    menu.scale_index = config.scale_index.min(ui::SCALE_NAMES.len() - 1);
    menu.bank_index = config.bank_index.min(ui::BANK_NAMES.len() - 1);
    menu.wt_volume = config.volume.min(100);
    menu.gr_volume = config.granular_volume;
    menu.oscillators_active = config.oscillators_active;
    menu.granular_active = config.granular_active;
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

    let mut visuals_level_tx = None;
    if config.hdmi_visuals_enabled {
        match try_spawn_visuals() {
            Ok(level_tx) => {
                menu.video_status = VideoStatus::On;
                visuals_level_tx = Some(level_tx);
                info!("HDMI visuals enabled");
            }
            Err(VisualsInitError::NoHdmi) => {
                menu.video_status = VideoStatus::NoHdmi;
                warn!("HDMI visuals enabled in config but no HDMI connector is connected");
            }
            Err(VisualsInitError::Init(err)) => {
                menu.video_status = VideoStatus::NoHdmi;
                warn!("HDMI visuals disabled after DRM/framebuffer init failure: {err:#}");
            }
        }
    } else {
        menu.video_status = VideoStatus::Off;
    }

    info!("initializing synth engine");
    let mut engine = initialize_engine(&config, initial_bank)?;
    info!("selected synthesis source: {:?}", engine.source_kind());
    let initial_hz = key_to_frequency_hz(menu.key_name(), menu.octave, 0.0)?;
    engine.set_frequency(initial_hz);
    apply_engine_params(&mut engine, &menu, &config);
    engine.set_scale(
        scale_mode_from_index(config.scale_index),
        config.fine_tune_cents,
    );
    engine.set_transition_secs(config.transition_secs);
    engine.set_wavetable_volume(config.volume);
    engine.set_oscillators_active_immediate(config.oscillators_active);
    engine.set_granular_volume(config.granular_volume);
    engine.set_granular_active_immediate(config.granular_active);
    info!("initial frequency set to {:.2} Hz", initial_hz);

    let (audio_tx, audio_rx) = command_channel();
    let mut synth = SynthController::new(audio_tx.clone(), 200);
    synth.set_note_transition_ms(config.note_transition_ms);
    info!("starting ALSA audio thread");
    let audio_handle = spawn_audio_thread(
        engine,
        AudioConfig {
            sample_rate: config.sample_rate,
            buffer_frames: config.buffer_frames,
            device: hw_alsa_device.clone(),
        },
        audio_rx,
        visuals_level_tx,
    );

    info!("initializing button GPIO inputs");
    let mut buttons = ButtonReader::new(hw_button_config)
        .context("failed to configure hardware button mapping")?;
    info!("button GPIO inputs initialized");
    let midi = match initialize_midi() {
        Ok(runtime) => runtime,
        Err(err) => {
            warn!("MIDI input disabled: {err}");
            None
        }
    };

    info!(
        "initializing {:?} display over {}",
        hw_profile, hw_spi_device
    );
    let mut display = hw_display;
    info!(
        "display initialized (DC=BCM{}, backlight={:?})",
        hw_dc_pin, hw_backlight_pin
    );

    info!("rendering initial menu frame");
    display.draw_menu(&menu)?;
    info!("startup complete");

    let hostname = std::fs::read_to_string("/etc/hostname")
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "pirate-synth".to_string());

    const IDLE_TIMEOUT: Duration = Duration::from_secs(30);
    let mut last_activity = Instant::now();
    let mut idle_mode = false;
    const SHUTDOWN_HOLD_DURATION: Duration = Duration::from_secs(5);
    let mut shutdown_combo_start: Option<Instant> = None;
    let mut raw_buf: Vec<bool> = Vec::with_capacity(4);

    loop {
        // Shutdown combo: Up + Down held together for 5 seconds triggers a safe shutdown.
        // Checked before the idle logic so the combo is not interrupted by the idle screen.
        buttons.raw_states_into(&mut raw_buf);
        if raw_buf.len() >= 2 && raw_buf[0] && raw_buf[1] {
            last_activity = Instant::now();
            match shutdown_combo_start {
                None => shutdown_combo_start = Some(Instant::now()),
                Some(start) if start.elapsed() >= SHUTDOWN_HOLD_DURATION => {
                    if request_shutdown(&mut display, &audio_tx) {
                        break;
                    } else {
                        shutdown_combo_start = None;
                        buttons.sync_state();
                        idle_mode = false;
                        if let Err(draw_err) = display.draw_menu(&menu) {
                            warn!("failed to restore display after shutdown failure: {draw_err}");
                        }
                    }
                }
                Some(_) => {}
            }
            synth.poll();
            std::thread::sleep(Duration::from_millis(25));
            continue;
        } else if shutdown_combo_start.is_some() {
            shutdown_combo_start = None;
            // Sync last-state so the next poll_pressed call does not see a spurious
            // rising edge from a button that was still held when the combo was cancelled.
            buttons.sync_state();
        }

        if buttons.poll_shutdown_pin() {
            last_activity = Instant::now();
            if request_shutdown(&mut display, &audio_tx) {
                break;
            }
            buttons.sync_state();
            idle_mode = false;
            if let Err(draw_err) = display.draw_menu(&menu) {
                warn!("failed to restore display after shutdown failure: {draw_err}");
            }
            synth.poll();
            std::thread::sleep(Duration::from_millis(25));
            continue;
        }

        // Idle timeout: switch to graphical overview screen
        if !idle_mode && last_activity.elapsed() >= IDLE_TIMEOUT {
            idle_mode = true;
            if let Err(err) = display.draw_idle_screen(&menu, &hostname) {
                warn!("failed to draw idle screen: {err}");
            }
        }

        if let Some(midi) = &midi {
            while let Ok(event) = midi.rx.try_recv() {
                let mut redraw = false;
                if idle_mode {
                    idle_mode = false;
                    redraw = true;
                }
                last_activity = Instant::now();
                match event {
                    MidiEvent::NoteOn(note) => {
                        let (next_key, next_octave) = midi_note_to_menu_key_octave(note);
                        if menu.key_index != next_key || menu.octave != next_octave {
                            menu.key_index = next_key;
                            menu.octave = next_octave;
                            redraw = true;
                            let hz = key_to_frequency_hz(menu.key_name(), menu.octave, 0.0)?;
                            synth.set_note_hz(hz);
                        }
                    }
                    MidiEvent::ControlChange { controller, value } => {
                        if controller == midi_cents_cc {
                            let cents = midi_cc_to_cents(value);
                            if (menu.fine_tune_cents - cents).abs() >= 0.01 {
                                menu.fine_tune_cents = cents;
                                synth.stage_fine_tune_cents(menu.fine_tune_cents);
                                redraw = true;
                            }
                        }
                    }
                }
                if redraw {
                    display.draw_menu(&menu)?;
                }
            }
        }

        let joystick_button = hw_joystick_reader
            .as_mut()
            .and_then(|jr| jr.poll_pressed());
        let button_press = buttons.poll_pressed()?.or(joystick_button);
        if let Some(button) = button_press {
            last_activity = Instant::now();

            // If idle, any key wakes the display and resumes the menu
            if idle_mode {
                idle_mode = false;
                display.draw_menu(&menu)?;
                std::thread::sleep(Duration::from_millis(25));
                continue;
            }

            debug!("button press: {:?}", button);
            let old_key = menu.key_name();
            let old_octave = menu.octave;
            let old_cents = menu.fine_tune_cents;
            let old_spread = menu.stereo_spread;
            let old_scale = menu.scale_index;
            let old_bank = menu.bank_index;
            let old_wt_volume = menu.wt_volume;
            let old_gr_volume = menu.gr_volume;
            let old_oscs = menu.oscillators_active;
            let old_granular_active = menu.granular_active;
            let old_osc_count = menu.osc_count;
            let old_gr_voices = menu.gr_voices;

            menu.apply_button(button);
            display.draw_menu(&menu)?;

            if menu.key_name() != old_key || menu.octave != old_octave {
                let hz = key_to_frequency_hz(menu.key_name(), menu.octave, 0.0)?;
                synth.set_note_hz(hz);
            }

            if menu.fine_tune_cents != old_cents {
                synth.stage_fine_tune_cents(menu.fine_tune_cents);
            }

            if menu.stereo_spread != old_spread {
                if let Err(err) =
                    audio_tx.try_send(AudioCommand::SetStereoSpread(menu.stereo_spread))
                {
                    warn!("failed to send stereo spread to audio thread: {err}");
                }
            }

            if menu.scale_index != old_scale {
                synth.stage_scale(
                    scale_mode_from_index(menu.scale_index),
                    menu.fine_tune_cents,
                );
            }

            if menu.bank_index != old_bank {
                let bank_name = ui::BANK_NAMES.get(menu.bank_index).copied().unwrap_or("A");
                match load_bank(&config.wavetable_dir, bank_name, config.oscillators) {
                    Ok(tables) => synth.stage_bank(std::sync::Arc::from(tables)),
                    Err(err) => warn!("failed to load wavetable bank {bank_name}: {err}"),
                }
            }

            if menu.wt_volume != old_wt_volume {
                if let Err(err) =
                    audio_tx.try_send(AudioCommand::SetWavetableVolume(menu.wt_volume))
                {
                    warn!("failed to send volume to audio thread: {err}");
                }
            }

            if menu.gr_volume != old_gr_volume {
                if let Err(err) = audio_tx.try_send(AudioCommand::SetGranularVolume(menu.gr_volume))
                {
                    warn!("failed to send granular volume: {err}");
                }
            }

            if menu.oscillators_active != old_oscs {
                if let Err(err) =
                    audio_tx.try_send(AudioCommand::SetOscillatorsActive(menu.oscillators_active))
                {
                    warn!("failed to send oscillators active to audio thread: {err}");
                }
            }

            if menu.granular_active != old_granular_active {
                if let Err(err) =
                    audio_tx.try_send(AudioCommand::SetGranularActive(menu.granular_active))
                {
                    warn!("failed to send granular active: {err}");
                }
            }

            if menu.osc_count != old_osc_count {
                if let Err(err) =
                    audio_tx.try_send(AudioCommand::SetOscillatorCount(menu.osc_count))
                {
                    warn!("failed to send oscillator count: {err}");
                }
            }

            if menu.gr_voices != old_gr_voices {
                if let Err(err) = audio_tx.try_send(AudioCommand::SetGranularVoices(menu.gr_voices))
                {
                    warn!("failed to send granular voices: {err}");
                }
            }
        }

        synth.poll();
        menu.glide_progress = synth.transition_progress();
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
        assert_eq!(config.root_key, "A");
        assert_eq!(config.root_octave, 1);
        assert_eq!(config.midi_cents_cc, 1);
        assert_eq!(config.stereo_spread, 100);
        assert_eq!(config.scale_index, 7);
        assert_eq!(config.volume, 50);
        assert_eq!(config.granular_volume, 50);
        assert_eq!(config.granular_active, false);
        assert_eq!(config.wav_dir, PathBuf::from("/var/lib/pirate-synth/WAV"));
        assert_eq!(config.granular_max_overlap, 16);
        assert_eq!(config.granular_wavs, 8);
        assert!(!config.hdmi_visuals_enabled);
    }

    #[test]
    fn load_config_defaults_feature_flags() {
        let path = PathBuf::from("/tmp/does-not-exist-pirate-synth.toml");
        let config = load_config(&path).unwrap();
        assert!(config.reverb_enabled);
        assert!((config.reverb_wet - 0.20).abs() < 0.001);
        assert!(config.tremolo_enabled);
        assert!((config.tremolo_depth - 0.35).abs() < 0.001);
        assert!(!config.hdmi_visuals_enabled);
    }
    #[test]
    fn apply_user_config_overrides_selected_fields() {
        let base = AppConfig::default();
        let user = UserConfig {
            root_key: Some("G".into()),
            root_octave: Some(4),
            midi_cents_cc: Some(74),
            fm_enabled: Some(true),
            hdmi_visuals_enabled: Some(true),
            ..UserConfig::default()
        };
        let merged = apply_user_config(base, user);
        assert_eq!(merged.root_key, "G");
        assert_eq!(merged.root_octave, 4);
        assert_eq!(merged.midi_cents_cc, 74);
        assert!(merged.fm_enabled);
        assert!(merged.hdmi_visuals_enabled);
        // unchanged fields retain defaults
        assert_eq!(merged.sample_rate, 48_000);
        assert_eq!(merged.oscillators, 8);
        assert!(merged.reverb_enabled);
    }

    #[test]
    fn parse_midi_event_handles_note_on_and_cc() {
        assert_eq!(
            parse_midi_event(&[0x90, 64, 100]),
            Some(MidiEvent::NoteOn(64))
        );
        assert_eq!(
            parse_midi_event(&[0xB3, 74, 127]),
            Some(MidiEvent::ControlChange {
                controller: 74,
                value: 127
            })
        );
        assert_eq!(parse_midi_event(&[0x90, 64, 0]), None);
    }

    #[test]
    fn midi_note_to_menu_key_octave_maps_and_clamps() {
        assert_eq!(midi_note_to_menu_key_octave(64), (4, 4)); // E4
        assert_eq!(midi_note_to_menu_key_octave(0), (0, 0));
        assert_eq!(midi_note_to_menu_key_octave(127), (7, 8));
    }

    #[test]
    fn midi_cc_to_cents_spans_expected_range() {
        assert!((midi_cc_to_cents(0) + 100.0).abs() < 0.001);
        assert!((midi_cc_to_cents(127) - 100.0).abs() < 0.001);
    }

    #[test]
    fn validate_midi_cc_enforces_standard_range() {
        assert_eq!(validate_midi_cc(0).unwrap(), 0);
        assert_eq!(validate_midi_cc(127).unwrap(), 127);
        assert!(validate_midi_cc(200).is_err());
    }

    #[test]
    fn config_default_note_transition_ms_is_zero() {
        let config = AppConfig::default();
        assert_eq!(config.note_transition_ms, 0.0);
    }

    #[test]
    fn config_note_transition_ms_roundtrip() {
        let toml = "note_transition_ms = 2500.0\n";
        let config: AppConfig = toml::from_str(toml).unwrap();
        assert!((config.note_transition_ms - 2500.0).abs() < 0.01);
    }

    #[test]
    fn apply_user_config_overrides_note_transition_ms() {
        let base = AppConfig::default();
        let user = UserConfig {
            note_transition_ms: Some(3000.0),
            ..UserConfig::default()
        };
        let merged = apply_user_config(base, user);
        assert!((merged.note_transition_ms - 3000.0).abs() < 0.01);
    }

    #[test]
    fn test_hardware_profile_pirate_audio_parses() {
        let config: AppConfig = toml::from_str("hardware_profile = \"pirate-audio\"\n").unwrap();
        assert_eq!(config.hardware_profile, "pirate-audio");
        assert_eq!(
            HardwareProfile::from_str("pirate-audio").unwrap(),
            HardwareProfile::PirateAudio
        );
    }

    #[test]
    fn test_hardware_profile_gpi_case_parses() {
        assert_eq!(
            HardwareProfile::from_str("gpi-case").unwrap(),
            HardwareProfile::GpiCase
        );
    }

    #[test]
    fn test_hardware_profile_unknown_returns_error() {
        assert!(HardwareProfile::from_str("foobar").is_err());
    }

    #[test]
    fn test_hardware_profile_default_is_pirate_audio() {
        let config: AppConfig = toml::from_str("").unwrap();
        assert_eq!(config.hardware_profile, "pirate-audio");
    }

    #[test]
    fn test_spi_device_override_respected() {
        let profile = HardwareProfile::PirateAudio;

        let default_cfg: AppConfig = toml::from_str("").unwrap();
        assert_eq!(
            resolve_spi_device(&profile, &default_cfg),
            "/dev/spidev0.1".to_string()
        );

        let override_cfg: AppConfig = toml::from_str("spi_device = \"/dev/spidev1.0\"\n").unwrap();
        assert_eq!(
            resolve_spi_device(&profile, &override_cfg),
            "/dev/spidev1.0".to_string()
        );
    }

    #[test]
    fn test_spi_device_gpi_case_default() {
        let profile = HardwareProfile::GpiCase;
        let cfg: AppConfig = toml::from_str("").unwrap();
        assert_eq!(
            resolve_spi_device(&profile, &cfg),
            "/dev/spidev0.0".to_string()
        );
    }

    #[test]
    fn test_spi_device_explicit_pirate_audio_default_respected_for_gpi_case() {
        // With the new Option<String> approach, explicitly setting spi_device to the
        // pirate-audio global default "/dev/spidev0.1" is honoured even for GpiCase.
        let profile = HardwareProfile::GpiCase;
        let cfg: AppConfig = toml::from_str("spi_device = \"/dev/spidev0.1\"\n").unwrap();
        assert_eq!(
            resolve_spi_device(&profile, &cfg),
            "/dev/spidev0.1".to_string(),
            "explicit spi_device override must be respected regardless of profile"
        );
    }

    #[test]
    fn test_spi_device_none_in_default_config() {
        let cfg = AppConfig::default();
        assert!(cfg.spi_device.is_none(), "default AppConfig must have spi_device = None");
    }

    #[test]
    fn test_hardware_profile_default_spi_device() {
        assert_eq!(HardwareProfile::PirateAudio.default_spi_device(), "/dev/spidev0.1");
        assert_eq!(HardwareProfile::GpiCase.default_spi_device(), "/dev/spidev0.0");
    }

    #[test]
    fn test_hardware_profile_default_dc_pin() {
        assert_eq!(resolve_dc_pin(&HardwareProfile::PirateAudio, &AppConfig::default()), 9);
        assert_eq!(resolve_dc_pin(&HardwareProfile::GpiCase, &AppConfig::default()), 25);
    }

    #[test]
    fn test_hardware_profile_dc_pin_override() {
        let mut cfg = AppConfig::default();
        cfg.dc_pin_override = Some(12);
        assert_eq!(resolve_dc_pin(&HardwareProfile::PirateAudio, &cfg), 12);
        assert_eq!(resolve_dc_pin(&HardwareProfile::GpiCase, &cfg), 12);
    }

    #[test]
    fn test_hardware_profile_backlight_pin() {
        assert_eq!(
            resolve_backlight_pin(&HardwareProfile::PirateAudio, &AppConfig::default()),
            Some(13)
        );
        assert_eq!(
            resolve_backlight_pin(&HardwareProfile::GpiCase, &AppConfig::default()),
            None
        );
    }

    #[test]
    fn test_hardware_profile_backlight_pin_override() {
        let mut cfg = AppConfig::default();
        cfg.backlight_pin_override = Some(22);
        assert_eq!(resolve_backlight_pin(&HardwareProfile::PirateAudio, &cfg), Some(22));
        assert_eq!(resolve_backlight_pin(&HardwareProfile::GpiCase, &cfg), Some(22));
    }

    #[test]
    fn test_scale_mode_from_index_covers_all_variants() {
        use engine::ScaleMode;
        assert!(matches!(scale_mode_from_index(0), ScaleMode::None));
        assert!(matches!(scale_mode_from_index(1), ScaleMode::Major));
        assert!(matches!(scale_mode_from_index(2), ScaleMode::NaturalMinor));
        assert!(matches!(scale_mode_from_index(3), ScaleMode::Pentatonic));
        assert!(matches!(scale_mode_from_index(4), ScaleMode::Dorian));
        assert!(matches!(scale_mode_from_index(5), ScaleMode::Mixolydian));
        assert!(matches!(scale_mode_from_index(6), ScaleMode::WholeTone));
        assert!(matches!(scale_mode_from_index(7), ScaleMode::Hirajoshi));
        assert!(matches!(scale_mode_from_index(8), ScaleMode::Lydian));
        // out-of-range falls back to None
        assert!(matches!(scale_mode_from_index(99), ScaleMode::None));
    }

    #[test]
    fn test_midi_note_to_menu_key_octave_boundary_cases() {
        // Note 0 = C-1, clamped octave to 0
        let (key, oct) = midi_note_to_menu_key_octave(0);
        assert_eq!(key, 0);
        assert_eq!(oct, 0);
        // Note 127 = G9, octave clamped to 8
        let (key, oct) = midi_note_to_menu_key_octave(127);
        assert_eq!(key, 7);
        assert_eq!(oct, 8);
        // Middle C = note 60 = C4
        let (key, oct) = midi_note_to_menu_key_octave(60);
        assert_eq!(key, 0); // C
        assert_eq!(oct, 4);
    }

    #[test]
    fn test_parse_midi_event_note_off_ignored() {
        // velocity 0 on note-on is treated as note-off; must return None
        assert_eq!(parse_midi_event(&[0x90, 64, 0]), None);
    }

    #[test]
    fn test_parse_midi_event_short_message_returns_none() {
        assert_eq!(parse_midi_event(&[0x90, 64]), None);
        assert_eq!(parse_midi_event(&[]), None);
    }

    #[test]
    fn test_parse_midi_event_channel_mask_applied() {
        // status byte 0x9F = note-on on channel 15; should still decode
        assert_eq!(parse_midi_event(&[0x9F, 60, 80]), Some(MidiEvent::NoteOn(60)));
    }

    #[test]
    fn test_granular_config_clamping() {
        let mut cfg = AppConfig::default();
        cfg.granular_channels = 0; // below minimum 1
        cfg.granular_pitch_cents = -9999.0; // below minimum -2400
        cfg.granular_position = 2.0; // above maximum 1.0
        let gc = granular_config(&cfg);
        assert_eq!(gc.granular_channels, 1, "granular_channels must be clamped to 1");
        assert!(gc.granular_pitch_cents >= -2400.0, "pitch_cents must be clamped");
        assert!(gc.position <= 1.0, "position must be clamped");
    }

    #[test]
    fn test_apply_user_config_spi_device_override() {
        let base = AppConfig::default();
        assert!(base.spi_device.is_none());
        let user = UserConfig {
            spi_device: Some("/dev/spidev1.0".into()),
            ..UserConfig::default()
        };
        let merged = apply_user_config(base, user);
        assert_eq!(merged.spi_device.as_deref(), Some("/dev/spidev1.0"));
    }

    #[test]
    fn test_apply_user_config_spi_device_none_preserves_base() {
        let mut base = AppConfig::default();
        base.spi_device = Some("/dev/spidev0.0".into());
        let user = UserConfig {
            spi_device: None,
            ..UserConfig::default()
        };
        let merged = apply_user_config(base, user);
        assert_eq!(merged.spi_device.as_deref(), Some("/dev/spidev0.0"));
    }
}
