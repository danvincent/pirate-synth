use thiserror::Error;

#[derive(Debug, Error)]
pub enum EngineError {
    #[error("wavetable must have at least 2 samples")]
    EmptyWavetable,
    #[error("granular source must have at least 2 samples")]
    EmptyGranularSource,
    #[error("oscillator count must be >= 1")]
    InvalidOscillatorCount,
}

#[derive(Clone, Debug)]
pub struct Wavetable {
    pub name: String,
    pub samples: Vec<f32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceKind {
    Wavetable,
    Wav,
}

#[derive(Clone, Copy, Debug)]
pub struct GranularConfig {
    pub grain_size_ms: f32,
    /// Total note duration in ms; grain loops its WAV window for this long before expiring.
    /// Use grain_size_ms as the looped window; grain_note_ms as overall lifespan.
    pub grain_note_ms: f32,
    /// Spawn interval jitter: ±this fraction of spawn interval is added randomly each spawn.
    pub spawn_jitter: f32,
    pub grain_density_hz: f32,
    pub max_overlapping_grains: usize,
    pub position: f32,
    pub position_jitter: f32,
    pub envelope_attack_ms: f32,
    pub envelope_release_ms: f32,
    pub scale_mode: ScaleMode,
    pub granular_channels: usize,
    pub granular_pitch_cents: f32,
}

impl Default for GranularConfig {
    fn default() -> Self {
        Self {
            grain_size_ms: 250.0,
            grain_note_ms: 4000.0,
            spawn_jitter: 0.5,
            grain_density_hz: 4.0,
            max_overlapping_grains: 16,
            position: 0.5,
            position_jitter: 0.15,
            envelope_attack_ms: 500.0,
            envelope_release_ms: 500.0,
            scale_mode: ScaleMode::None,
            granular_channels: 4,
            granular_pitch_cents: 1200.0,
        }
    }
}

#[derive(Clone, Debug)]
pub struct GranularSource {
    pub name: String,
    pub sample_rate: u32,
    pub samples: Vec<f32>,
}

/// Pre-built bytebeat formulas.  Each formula takes an integer time counter `t`
/// (advancing at 8 kHz) and returns a raw `u8` sample value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BytebeatAlgo {
    /// `t & (t >> 8)` – simple harmonic wave
    Basic,
    /// `(t * 5 & (t >> 7)) | (t * 3 & (t >> 10))` – Sierpinski-like texture
    Sierpinski,
    /// `t * (t >> 10 | t >> 8) & 63 & (t >> 4)` – arpeggiated melody
    Melody,
    /// `t | (t >> 3) | (t >> 5) | (t >> 7)` – rich harmonic drone
    Harmony,
    /// `t * (t >> 7 | t >> 14)` – acid bass texture
    Acid,
}

impl BytebeatAlgo {
    pub fn eval(self, t: u64) -> u8 {
        match self {
            BytebeatAlgo::Basic => (t & (t >> 8)) as u8,
            BytebeatAlgo::Sierpinski => {
                ((t.wrapping_mul(5) & (t >> 7)) | (t.wrapping_mul(3) & (t >> 10))) as u8
            }
            BytebeatAlgo::Melody => {
                (t.wrapping_mul(t >> 10 | t >> 8) & 63 & (t >> 4)) as u8
            }
            BytebeatAlgo::Harmony => (t | (t >> 3) | (t >> 5) | (t >> 7)) as u8,
            BytebeatAlgo::Acid => t.wrapping_mul(t >> 7 | t >> 14) as u8,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            BytebeatAlgo::Basic => "Basic",
            BytebeatAlgo::Sierpinski => "Sierpinski",
            BytebeatAlgo::Melody => "Melody",
            BytebeatAlgo::Harmony => "Harmony",
            BytebeatAlgo::Acid => "Acid",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ScaleMode {
    None,
    Major,
    NaturalMinor,
    Pentatonic,
    Dorian,
    Mixolydian,
    WholeTone,
    Hirajoshi,
    Lydian,
}

impl ScaleMode {
    /// Semitone offsets from root (within one octave)
    pub fn semitones(&self) -> &'static [i32] {
        match self {
            ScaleMode::None => &[],
            ScaleMode::Major => &[0, 2, 4, 5, 7, 9, 11],
            ScaleMode::NaturalMinor => &[0, 2, 3, 5, 7, 8, 10],
            ScaleMode::Pentatonic => &[0, 2, 4, 7, 9],
            ScaleMode::Dorian => &[0, 2, 3, 5, 7, 9, 10],
            ScaleMode::Mixolydian => &[0, 2, 4, 5, 7, 9, 10],
            ScaleMode::WholeTone => &[0, 2, 4, 6, 8, 10],
            ScaleMode::Hirajoshi => &[0, 2, 3, 7, 8],
            ScaleMode::Lydian => &[0, 2, 4, 6, 7, 9, 11],
        }
    }
}
