use crate::audio::dsp::FrameProcessor;
use crate::audio::presets::lerp_struct;
use serde::{Deserialize, Serialize};

lerp_struct! {
    #[derive(Deserialize, Serialize, Clone, Copy, Debug, garde::Validate)]
    pub struct Clip {
        #[garde(range(min = 0.0, max = 100.0))]
        pub pregain: f32,
        #[garde(range(min = 0.0, max = 2.0))]
        pub threshold: f32,
    }
}

fn soft_clip(sample: f32, threshold: f32) -> f32 {
    if sample.abs() < threshold {
        sample
    } else {
        threshold * sample.signum() * (1.0 - (-sample.abs() / threshold).exp())
    }
}

impl FrameProcessor for Clip {
    fn process_frame(&mut self, frame: [f32; 2]) -> [f32; 2] {
        frame.map(|sample| soft_clip(sample * self.pregain, self.threshold))
    }
}
