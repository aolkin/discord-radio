use crate::audio::dsp::FrameProcessor;
use crate::audio::presets::lerp_by_step;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Clone, Copy, Debug, garde::Validate)]
pub struct Bitcrush {
    #[garde(range(min = 1, max = 16))]
    pub bits: u8,
}

lerp_by_step!(Option<Bitcrush>);

impl FrameProcessor for Bitcrush {
    fn process_frame(&mut self, frame: [f32; 2]) -> [f32; 2] {
        let step_size = 1.0 / 2.0_f32.powi(self.bits as i32 - 1);
        frame.map(|sample| (sample / step_size).round() * step_size)
    }
}
