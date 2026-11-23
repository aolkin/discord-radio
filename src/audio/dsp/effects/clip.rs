use crate::audio::dsp::FrameProcessor;
use crate::audio::presets::{check, lerp_struct};
use serde::{Deserialize, Serialize};

const CLIP_PREGAIN: (f32, f32) = (0.0, 100.0);
const CLIP_THRESHOLD: (f32, f32) = (0.0, 2.0);

lerp_struct! {
    #[derive(Deserialize, Serialize, Clone, Copy, Debug)]
    pub struct Clip {
        pub pregain: f32,
        pub threshold: f32,
    }
}

impl Clip {
    pub fn check_limits(&self) -> anyhow::Result<()> {
        check("clip.pregain", self.pregain, CLIP_PREGAIN)?;
        check("clip.threshold", self.threshold, CLIP_THRESHOLD)
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
