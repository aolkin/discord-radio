use crate::audio::dsp::{Effect, FrameProcessor};
use crate::audio::presets::lerp_struct;
use serde::{Deserialize, Serialize};

lerp_struct! {
    #[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq, garde::Validate)]
    pub struct ClipParams {
        #[garde(range(min = 0.0, max = 100.0))]
        pub pregain: f32,
        #[garde(range(min = 0.0, max = 2.0))]
        pub threshold: f32,
    }
}

pub struct Clip {
    params: ClipParams,
}

impl Effect for Clip {
    type Params = ClipParams;

    fn new(_sample_rate: u32, params: &ClipParams) -> Self {
        Self { params: *params }
    }

    fn update(&mut self, params: &ClipParams) {
        self.params = *params;
    }
}

impl FrameProcessor for Clip {
    fn process_frame(&mut self, frame: [f32; 2]) -> [f32; 2] {
        frame.map(|sample| soft_clip(sample * self.params.pregain, self.params.threshold))
    }
}

fn soft_clip(sample: f32, threshold: f32) -> f32 {
    if sample.abs() < threshold {
        sample
    } else {
        threshold * sample.signum() * (1.0 - (-sample.abs() / threshold).exp())
    }
}
