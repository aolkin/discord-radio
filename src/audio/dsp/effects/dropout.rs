use crate::audio::dsp::FrameProcessor;
use crate::audio::presets::Dropout;
use rand::{RngExt, rngs::StdRng};

pub struct DropoutGenerator {
    dropout: Dropout,
    sample_rate: f32,
    frames_until_next_check: u32,
    dropout_frames_remaining: u32,
    rng: StdRng,
}

impl DropoutGenerator {
    pub fn new(dropout: &Dropout, sample_rate: u32) -> Self {
        Self {
            dropout: *dropout,
            sample_rate: sample_rate as f32,
            frames_until_next_check: 0,
            dropout_frames_remaining: 0,
            rng: rand::make_rng(),
        }
    }

    pub fn update(&mut self, dropout: &Dropout) {
        self.dropout = *dropout;
    }

    fn should_dropout(&mut self) -> bool {
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

        let probability = self.dropout.probability / 10.0;

        if self.rng.random::<f32>() < probability {
            let duration_ms = self
                .rng
                .random_range(self.dropout.min_ms..=self.dropout.max_ms);
            let duration_frames = (duration_ms / 1000.0 * self.sample_rate) as u32;
            self.dropout_frames_remaining = duration_frames;
            return true;
        }

        false
    }
}

impl FrameProcessor for DropoutGenerator {
    fn process_frame(&mut self, frame: [f32; 2]) -> [f32; 2] {
        if self.should_dropout() {
            [0.0, 0.0]
        } else {
            frame
        }
    }
}
