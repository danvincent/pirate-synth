pub(crate) struct CombFilter {
    buffer: Vec<f32>,
    pos: usize,
    feedback: f32,
    damp: f32,
    damp_state: f32,
}

impl CombFilter {
    pub(crate) fn new(delay_samples: usize, feedback: f32, damp: f32) -> Self {
        Self {
            buffer: vec![0.0; delay_samples],
            pos: 0,
            feedback,
            damp,
            damp_state: 0.0,
        }
    }

    pub(crate) fn process(&mut self, input: f32) -> f32 {
        let out = self.buffer[self.pos];
        self.damp_state = out * (1.0 - self.damp) + self.damp_state * self.damp;
        self.buffer[self.pos] = input + self.damp_state * self.feedback;
        self.pos = (self.pos + 1) % self.buffer.len();
        out
    }
}

pub(crate) struct AllpassFilter {
    buffer: Vec<f32>,
    pos: usize,
}

impl AllpassFilter {
    pub(crate) fn new(delay_samples: usize) -> Self {
        Self {
            buffer: vec![0.0; delay_samples],
            pos: 0,
        }
    }

    pub(crate) fn process(&mut self, input: f32) -> f32 {
        let buf = self.buffer[self.pos];
        let out = -input + buf;
        self.buffer[self.pos] = input + buf * 0.5;
        self.pos = (self.pos + 1) % self.buffer.len();
        out
    }
}

pub(crate) struct Reverb {
    combs: Vec<CombFilter>,
    allpasses: [AllpassFilter; 2],
}

impl Reverb {
    /// short = true → short room (odd bus); short = false → long room (even bus)
    /// Uses default: feedback=0.84, damp=0.20, comb_count=4
    pub(crate) fn new(short: bool) -> Self {
        Self::new_with_params(short, 0.84, 0.20, 4)
    }

    /// Create a reverb with configurable parameters.
    /// 
    /// # Arguments
    /// * `short` - true for short room (scale 1.0), false for long room (scale 1.25)
    /// * `feedback` - feedback coefficient (0.0–0.97); higher = longer tail
    /// * `damp` - damping coefficient (0.0–1.0); higher = more high-frequency rolloff
    /// * `comb_count` - number of comb filters (1–8); clamped to this range
    pub(crate) fn new_with_params(short: bool, feedback: f32, damp: f32, comb_count: usize) -> Self {
        let feedback = feedback.clamp(0.0, 0.97);
        let damp = damp.clamp(0.0, 1.0);
        let scale = if short { 1.0f32 } else { 1.25f32 };
        let clamped_count = comb_count.clamp(1, 8);
        // Base comb delays (samples at 48kHz)
        let delays = [1116usize, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
        
        let mut combs = Vec::with_capacity(clamped_count);
        for i in 0..clamped_count {
            let delay_samples = (delays[i] as f32 * scale) as usize;
            combs.push(CombFilter::new(delay_samples, feedback, damp));
        }
        
        let allpasses = [AllpassFilter::new(556), AllpassFilter::new(441)];
        Self { combs, allpasses }
    }

    pub(crate) fn process(&mut self, input: f32) -> f32 {
        let mut comb_sum = 0.0f32;
        for comb in &mut self.combs {
            comb_sum += comb.process(input);
        }
        let ap1 = self.allpasses[0].process(comb_sum);
        self.allpasses[1].process(ap1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reverb_new_with_params_clamps_comb_count() {
        // Should not panic with comb_count=0
        let _rev0 = Reverb::new_with_params(true, 0.84, 0.20, 0);
        
        // Should not panic with comb_count=10 (clamped to 8)
        let _rev10 = Reverb::new_with_params(true, 0.84, 0.20, 10);
    }

    #[test]
    fn reverb_produces_nonzero_output() {
        let mut reverb = Reverb::new(true);
        
        // Feed a single impulse
        let _impulse_output = reverb.process(1.0);
        
        // Process silence for many samples to let the reverb tail develop
        let mut output_after_samples = 0.0f32;
        for _ in 0..5000 {
            output_after_samples += reverb.process(0.0).abs();
        }
        
        // The reverb should produce some non-zero output after the impulse
        assert!(output_after_samples > 0.0001, "Reverb should produce output after impulse (got {})", output_after_samples);
    }

    #[test]
    fn reverb_feedback_affects_decay() {
        // Create two reverbs with different feedback values
        let mut reverb_low = Reverb::new_with_params(true, 0.5, 0.20, 4);
        let mut reverb_high = Reverb::new_with_params(true, 0.9, 0.20, 4);
        
        // Feed same impulse to both
        reverb_low.process(1.0);
        reverb_high.process(1.0);
        
        // Run 5000 samples and accumulate energy
        let mut energy_low = 0.0f32;
        let mut energy_high = 0.0f32;
        for _ in 0..5000 {
            energy_low += reverb_low.process(0.0).abs();
            energy_high += reverb_high.process(0.0).abs();
        }
        
        // Higher feedback should produce higher total energy due to longer decay
        assert!(energy_high > energy_low, 
            "Reverb with feedback=0.9 should have more energy than feedback=0.5 (got energy_high={} vs energy_low={})", 
            energy_high, energy_low);
    }
}
