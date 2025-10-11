use rand::{Rng, SeedableRng, rngs::StdRng};

#[allow(clippy::upper_case_acronyms)]
pub struct LFO {
    phase: f64,
    phase_increment: f64,
}

impl LFO {
    pub fn new(sample_rate: u32, frequency: f32) -> Self {
        let phase_increment = (frequency as f64) / (sample_rate as f64);
        Self {
            phase: 0.0,
            phase_increment,
        }
    }

    pub fn next(&mut self) -> f32 {
        let value = (self.phase * 2.0 * std::f64::consts::PI).sin();
        self.phase += self.phase_increment;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        value as f32
    }
}

pub struct Bitcrusher {
    depth: u8,
}

impl Bitcrusher {
    pub fn new(depth: u8) -> Self {
        Self { depth }
    }

    pub fn set_depth(&mut self, depth: u8) {
        self.depth = depth;
    }

    pub fn process(&self, sample: f32) -> f32 {
        let levels = 2.0_f32.powi(self.depth as i32);
        (sample * levels).round() / levels
    }

    pub fn process_frame(&self, frame: [f32; 2]) -> [f32; 2] {
        [self.process(frame[0]), self.process(frame[1])]
    }
}

pub struct DropoutGenerator {
    probability_per_second: f32,
    duration_range_ms: (f32, f32),
    sample_rate: f32,
    frames_until_next_check: u32,
    dropout_frames_remaining: u32,
    rng: StdRng,
}

impl DropoutGenerator {
    pub fn new(
        probability_per_second: f32,
        duration_range_ms: (f32, f32),
        sample_rate: u32,
    ) -> Self {
        use rand::RngCore;
        let mut seed_rng = rand::rng();
        let mut seed = [0u8; 32];
        seed_rng.fill_bytes(&mut seed);

        Self {
            probability_per_second,
            duration_range_ms,
            sample_rate: sample_rate as f32,
            frames_until_next_check: 0,
            dropout_frames_remaining: 0,
            rng: StdRng::from_seed(seed),
        }
    }

    pub fn should_dropout(&mut self) -> bool {
        if self.dropout_frames_remaining > 0 {
            self.dropout_frames_remaining -= 1;
            return true;
        }

        if self.frames_until_next_check > 0 {
            self.frames_until_next_check -= 1;
            return false;
        }

        let check_interval_frames = self.sample_rate / 10.0;
        self.frames_until_next_check = check_interval_frames as u32;

        let probability = self.probability_per_second / 10.0;

        if self.rng.random::<f32>() < probability {
            let duration_ms = self
                .rng
                .random_range(self.duration_range_ms.0..=self.duration_range_ms.1);
            let duration_frames = (duration_ms / 1000.0 * self.sample_rate) as u32;
            self.dropout_frames_remaining = duration_frames;
            return true;
        }

        false
    }
}
