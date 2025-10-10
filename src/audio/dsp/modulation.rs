use atomic_float::AtomicF32;
use rand::{rngs::StdRng, Rng, SeedableRng};
use std::collections::VecDeque;
use std::sync::atomic::Ordering;
use std::sync::Arc;

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

pub struct PitchShifter {
    pub cents: Arc<AtomicF32>,
    buffer: VecDeque<[f32; 2]>,
    buffer_size: usize,
    read_pos: f32,
}

impl PitchShifter {
    pub fn new(cents: Arc<AtomicF32>) -> Self {
        Self {
            cents,
            buffer: VecDeque::with_capacity(4096),
            buffer_size: 2048,
            read_pos: 0.0,
        }
    }

    pub fn process(&mut self, frame: [f32; 2]) -> [f32; 2] {
        self.buffer.push_back(frame);

        if self.buffer.len() > self.buffer_size {
            self.buffer.pop_front();
        }

        if self.buffer.len() < self.buffer_size {
            return frame;
        }

        let cents = self.cents.load(Ordering::Relaxed);
        let pitch_ratio = 2.0_f32.powf(cents / 1200.0);

        self.read_pos += pitch_ratio;

        if self.read_pos >= self.buffer.len() as f32 {
            self.read_pos -= self.buffer.len() as f32;
        }

        let idx = self.read_pos as usize;
        let frac = self.read_pos - idx as f32;

        if idx + 1 < self.buffer.len() {
            let frame1 = self.buffer[idx];
            let frame2 = self.buffer[idx + 1];

            [
                frame1[0] * (1.0 - frac) + frame2[0] * frac,
                frame1[1] * (1.0 - frac) + frame2[1] * frac,
            ]
        } else {
            frame
        }
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
            let duration_ms = self.rng.random_range(self.duration_range_ms.0..=self.duration_range_ms.1);
            let duration_frames = (duration_ms / 1000.0 * self.sample_rate) as u32;
            self.dropout_frames_remaining = duration_frames;
            return true;
        }

        false
    }
}
