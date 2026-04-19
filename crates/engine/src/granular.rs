use crate::types::{GranularConfig, GranularSource};
use crate::{lcg_next, Oscillator, Voice, C2_FREQUENCY_HZ};

#[derive(Clone, Debug)]
pub(crate) struct ActiveGrain {
    pub(crate) source_index: usize,
    pub(crate) start_sample: f32,
    pub(crate) sample_offset: f32,
    pub(crate) playback_ratio: f32,
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
            rng_state,
            initialized: false,
        }
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

    let osc_idx = granular.round_robin_counter % oscillators.len();
    let lane = granular.round_robin_counter % granular.configured_wavs;
    granular.round_robin_counter = granular.round_robin_counter.wrapping_add(1);
    let source_index = (granular.source_offset + lane) % granular.sources.len();
    let source = &granular.sources[source_index];
    let source_len = source.samples.len();
    if source_len < 2 {
        return;
    }

    let osc = &mut oscillators[osc_idx];

    // Randomise grain lifespan between min and max so multiple voices expire at different times.
    let note_ms = {
        let min = granular.config.grain_note_ms_min;
        let max = granular.config.grain_note_ms_max;
        if max > min {
            let t = lcg_next(&mut granular.rng_state) as f32 / u32::MAX as f32;
            min + t * (max - min)
        } else if min > 0.0 {
            min
        } else {
            granular.config.grain_size_ms
        }
    };
    let note_len_samples =
        ((note_ms.max(1.0) / 1000.0) * output_sample_rate) as usize;
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
    let avail = source_len.saturating_sub(start_sample as usize).saturating_sub(2);
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
    fn grain_detune_clamped_within_bounds() {
        let apply = |cents: f32| -> f32 {
            let folded = (cents + 700.0).rem_euclid(1200.0) - 700.0;
            2.0f32.powf(folded / 1200.0)
        };

        // +500 cents wraps to -700 (the fold boundary, half-open at +500)
        assert!((apply(500.0) - 2.0f32.powf(-700.0/1200.0)).abs() < 1e-4, "+5st should fold to -7st boundary");
        // +499 cents stays near +499 (just inside boundary)
        assert!(apply(499.0) > 2.0f32.powf(498.0/1200.0), "+499 cents should be positive detune");
    }
}
