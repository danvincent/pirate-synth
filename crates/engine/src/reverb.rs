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
    combs: [CombFilter; 4],
    allpasses: [AllpassFilter; 2],
}

impl Reverb {
    /// short = true → short room (odd bus); short = false → long room (even bus)
    pub(crate) fn new(short: bool) -> Self {
        let scale = if short { 1.0f32 } else { 1.25f32 };
        let feedback = 0.84f32;
        let damp = 0.20f32;
        // Base comb delays (samples at 48kHz)
        let delays = [1116usize, 1188, 1277, 1356];
        let combs = [
            CombFilter::new((delays[0] as f32 * scale) as usize, feedback, damp),
            CombFilter::new((delays[1] as f32 * scale) as usize, feedback, damp),
            CombFilter::new((delays[2] as f32 * scale) as usize, feedback, damp),
            CombFilter::new((delays[3] as f32 * scale) as usize, feedback, damp),
        ];
        let allpasses = [AllpassFilter::new(556), AllpassFilter::new(441)];
        Self { combs, allpasses }
    }

    pub(crate) fn process(&mut self, input: f32) -> f32 {
        let comb_sum = self.combs[0].process(input)
            + self.combs[1].process(input)
            + self.combs[2].process(input)
            + self.combs[3].process(input);
        let ap1 = self.allpasses[0].process(comb_sum);
        self.allpasses[1].process(ap1)
    }
}
