use crate::types::{GranularConfig, GranularSource};
use crate::{lcg_next, Oscillator, Voice, C2_FREQUENCY_HZ};

#[derive(Clone, Debug)]
pub(crate) struct GranularChannel {
    pub(crate) detune_ratio: f32,
    pub(crate) source_index: usize,
}

#[derive(Clone, Debug)]
pub(crate) struct ActiveGrain {
    pub(crate) source_index: usize,
    pub(crate) start_sample: f32,
    pub(crate) sample_offset: f32,
    pub(crate) playback_ratio: f32,
    pub(crate) detune_ratio: f32,
    pub(crate) sample_length: usize,
    pub(crate) window_source_samples: usize,
    pub(crate) age_samples: usize,
    pub(crate) attack_samples: usize,
    pub(crate) release_samples: usize,
    pub(crate) voice: Voice,
    pub(crate) rng_state: u64,
}

#[derive(Clone, Debug)]
pub(crate) struct GranularState {
    pub(crate) sources: Vec<GranularSource>,
    pub(crate) source_voices: Vec<Voice>,
    pub(crate) config: GranularConfig,
    pub(crate) samples_until_next_grain: f32,
    pub(crate) active_grains: Vec<ActiveGrain>,
    pub(crate) source_offset: usize,
    pub(crate) configured_wavs: usize,
    pub(crate) round_robin_counter: usize,
    pub(crate) channels: Vec<GranularChannel>,
    pub(crate) channel_counter: usize,
    pub(crate) rng_state: u64,
    pub(crate) initialized: bool,
}

impl GranularState {
    pub(crate) fn new(sources: Vec<GranularSource>, config: GranularConfig) -> Self {
        let configured_wavs = sources.len();
        // Seed rng from source sample counts so each load has a unique phase.
        let rng_state = sources.iter().fold(0xdeadbeef_cafebabe_u64, |acc, s| {
            acc.wrapping_add(s.samples.len() as u64)
                .wrapping_mul(6364136223846793005_u64)
        });

        // Initialize source_voices with stereo panning (-1 = full left, 0 = center, 1 = full right)
        let source_voices = sources
            .iter()
            .enumerate()
            .map(|(i, _)| {
                let n = sources.len();
                let pan_pos = if n <= 1 {
                    0.0f32
                } else {
                    -1.0 + 2.0 * i as f32 / (n - 1) as f32
                };
                let angle = (pan_pos + 1.0) * std::f32::consts::FRAC_PI_4;
                Voice::new(angle.cos(), angle.sin())
            })
            .collect();

        Self {
            sources,
            source_voices,
            config,
            samples_until_next_grain: 0.0,
            active_grains: Vec::new(),
            source_offset: 0,
            configured_wavs,
            round_robin_counter: 0,
            channels: Vec::new(),
            channel_counter: 0,
            rng_state,
            initialized: false,
        }
    }
}

pub(crate) fn assign_channels(
    state: &mut GranularState,
    config: &GranularConfig,
    n_sources: usize,
    rng_seed: u64,
) {
    state.channels.clear();
    state.channel_counter = 0;

    if n_sources == 0 || config.granular_channels == 0 {
        return;
    }

    state.channels.reserve(config.granular_channels);
    let mut rng_state = rng_seed;
    let scale_notes = config.scale_mode.semitones();
    let half_spread_semitones = (config.granular_pitch_cents.max(0.0) / 200.0).max(0.0);

    let (min_note, max_note) = if scale_notes.is_empty() {
        (0_i32, 0_i32)
    } else {
        let mut min_note = i32::MAX;
        let mut max_note = i32::MIN;
        for &note in scale_notes {
            min_note = min_note.min(note);
            max_note = max_note.max(note);
        }
        (min_note, max_note)
    };

    for _ in 0..config.granular_channels {
        let semitone_offset = if scale_notes.is_empty() {
            0.0
        } else {
            let note_index = lcg_next(&mut rng_state) as usize % scale_notes.len();
            let note = scale_notes[note_index] as f32;
            let spread = (max_note - min_note) as f32;
            if spread <= 0.0 {
                0.0
            } else {
                let normalized = (note - min_note as f32) / spread;
                (normalized * 2.0 - 1.0) * half_spread_semitones
            }
        };

        let detune_ratio = 2.0f32.powf(semitone_offset / 12.0);
        let source_index = lcg_next(&mut rng_state) as usize % n_sources;
        state.channels.push(GranularChannel {
            detune_ratio,
            source_index,
        });
    }
}

pub(crate) fn spawn_grain(
    granular: &mut GranularState,
    oscillators: &mut [Oscillator],
    output_sample_rate: f32,
    base_frequency_hz: f32,
    fine_tune_cents: f32,
) {
    if granular.sources.is_empty() || oscillators.is_empty() {
        return;
    }
    if granular.configured_wavs == 0 {
        return;
    }
    if granular.active_grains.len() >= granular.config.max_overlapping_grains.max(1) {
        return;
    }

    let round_robin_counter = granular.round_robin_counter;
    let osc_idx = round_robin_counter % oscillators.len();
    granular.round_robin_counter = granular.round_robin_counter.wrapping_add(1);
    let (source_index, detune_ratio) = if granular.channels.is_empty() {
        let lane = round_robin_counter % granular.configured_wavs;
        (
            (granular.source_offset + lane) % granular.sources.len(),
            1.0,
        )
    } else {
        let channel_idx = granular.channel_counter % granular.channels.len();
        granular.channel_counter = granular.channel_counter.wrapping_add(1);
        let channel = &granular.channels[channel_idx];
        let active = granular.configured_wavs.max(1).min(granular.sources.len());
        let source_index =
            (granular.source_offset + channel.source_index % active) % granular.sources.len();
        (source_index, channel.detune_ratio)
    };
    debug_assert!(source_index < granular.sources.len());
    let source = &granular.sources[source_index];
    let source_len = source.samples.len();
    if source_len < 2 {
        return;
    }

    let osc = &mut oscillators[osc_idx];

    // grain_size_ms is the WAV window size (texture chunk); grain_note_ms is the total
    // note lifespan. If grain_note_ms is 0 or unset, fall back to grain_size_ms.
    let note_ms = if granular.config.grain_note_ms > 0.0 {
        granular.config.grain_note_ms
    } else {
        granular.config.grain_size_ms
    };
    let note_len_samples = ((note_ms.max(1.0) / 1000.0) * output_sample_rate) as usize;
    let note_len_samples = note_len_samples.max(8);

    // Compute the source-space window this grain loops through.
    let window_source_samples =
        ((granular.config.grain_size_ms.max(1.0) / 1000.0) * source.sample_rate as f32) as usize;

    let jitter = (lcg_next(&mut osc.rng_state) as f32 / u32::MAX as f32) * 2.0 - 1.0;
    let grain_rng_state = osc.rng_state;
    let position = (granular.config.position + jitter * granular.config.position_jitter.max(0.0))
        .clamp(0.0, 1.0);

    // Clamp window to what's actually available in the source from start_sample.
    let max_start = source_len.saturating_sub(window_source_samples.max(2) + 1);
    let start_sample = position * max_start as f32;
    let avail = source_len
        .saturating_sub(start_sample as usize)
        .saturating_sub(2);
    let window_source_samples = window_source_samples.min(avail).max(1);

    // Base C2 is used by the wavetable drone path, so we keep pitch relationships aligned.
    let root_ratio = (base_frequency_hz / C2_FREQUENCY_HZ).max(0.01);
    let fine_ratio = 2.0f32.powf(fine_tune_cents / 1200.0);

    // Fold oscillator detune into one octave, producing the half-open range [-700, +500) cents
    // ([-7, +5) semitones). This prevents chipmunk-style pitch jumps when the oscillator
    // detune spans multiple octaves from scale/spread logic.
    let detune_cents = 1200.0 * osc.detune_ratio.max(f32::MIN_POSITIVE).log2();
    let folded_cents = (detune_cents + 700.0).rem_euclid(1200.0) - 700.0;
    let grain_detune_ratio = 2.0f32.powf(folded_cents / 1200.0);

    let playback_ratio = root_ratio
        * fine_ratio
        * grain_detune_ratio
        * (source.sample_rate as f32 / output_sample_rate);

    let attack =
        ((granular.config.envelope_attack_ms.max(0.0) / 1000.0) * output_sample_rate) as usize;
    let release =
        ((granular.config.envelope_release_ms.max(0.0) / 1000.0) * output_sample_rate) as usize;

    granular.active_grains.push(ActiveGrain {
        source_index,
        start_sample,
        sample_offset: 0.0,
        playback_ratio,
        detune_ratio,
        sample_length: note_len_samples,
        window_source_samples,
        age_samples: 0,
        attack_samples: attack,
        release_samples: release,
        voice: {
            let sv = &granular.source_voices[source_index];
            Voice::new(sv.pan_l, sv.pan_r)
        },
        rng_state: grain_rng_state,
    });
}

pub(crate) fn grain_envelope(grain: &ActiveGrain) -> f32 {
    let attack = grain.attack_samples.max(1);
    let release = grain.release_samples.max(1);
    let age = grain.age_samples;
    let remaining = grain.sample_length.saturating_sub(age);

    if age < attack {
        return age as f32 / attack as f32;
    }
    if remaining < release {
        return remaining as f32 / release as f32;
    }
    1.0
}

pub(crate) fn sample_linear(samples: &[f32], pos: f32) -> f32 {
    let i0 = pos.floor() as usize;
    let i1 = (i0 + 1).min(samples.len().saturating_sub(1));
    let frac = (pos - i0 as f32).clamp(0.0, 1.0);
    samples[i0] * (1.0 - frac) + samples[i1] * frac
}

#[cfg(test)]
mod tests {
    #[test]
    fn detune_ratio_should_fold_to_lower_boundary_when_cents_is_exactly_500() {
        let apply = |cents: f32| -> f32 {
            let folded = (cents + 700.0).rem_euclid(1200.0) - 700.0;
            2.0f32.powf(folded / 1200.0)
        };

        // +500 cents wraps to -700 (the fold boundary, half-open at +500)
        assert!(
            (apply(500.0) - 2.0f32.powf(-700.0 / 1200.0)).abs() < 1e-4,
            "+5st should fold to -7st boundary"
        );
    }

    #[test]
    fn detune_ratio_should_remain_positive_when_cents_is_just_below_500() {
        let apply = |cents: f32| -> f32 {
            let folded = (cents + 700.0).rem_euclid(1200.0) - 700.0;
            2.0f32.powf(folded / 1200.0)
        };

        // +499 cents stays near +499 (just inside boundary)
        assert!(
            apply(499.0) > 2.0f32.powf(498.0 / 1200.0),
            "+499 cents should be positive detune"
        );
    }
}
