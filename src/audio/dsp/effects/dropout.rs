use crate::audio::dsp::{Effect, FrameProcessor};
use crate::audio::presets::lerp_struct;
use rand::{RngExt, rngs::StdRng};
use serde::{Deserialize, Serialize};

lerp_struct! {
    #[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq, garde::Validate)]
    pub struct DropoutParams {
        #[garde(range(min = 0.0, max = 1.0))]
        pub probability: f32,
        #[garde(range(min = 0.0, max = 2000.0))]
        pub min_ms: f32,
        #[garde(range(min = self.min_ms, max = 2000.0))]
        pub max_ms: f32,
    }
}

pub struct Dropout {
    params: DropoutParams,
    sample_rate: f32,
    frames_until_next_check: u32,
    dropout_frames_remaining: u32,
    rng: StdRng,
}

impl Dropout {
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

        let probability = self.params.probability / 10.0;

        if self.rng.random::<f32>() < probability {
            let duration_ms = self
                .rng
                .random_range(self.params.min_ms..=self.params.max_ms);
            let duration_frames = (duration_ms / 1000.0 * self.sample_rate) as u32;
            self.dropout_frames_remaining = duration_frames;
            return true;
        }

        false
    }
}

impl Effect for Dropout {
    type Params = DropoutParams;

    fn new(sample_rate: u32, params: &DropoutParams) -> Self {
        Self {
            params: *params,
            sample_rate: sample_rate as f32,
            frames_until_next_check: 0,
            dropout_frames_remaining: 0,
            rng: rand::make_rng(),
        }
    }

    fn update(&mut self, params: &DropoutParams) {
        self.params = *params;
    }
}

impl FrameProcessor for Dropout {
    fn process_frame(&mut self, frame: [f32; 2]) -> [f32; 2] {
        if self.should_dropout() {
            [0.0, 0.0]
        } else {
            frame
        }
    }
}
