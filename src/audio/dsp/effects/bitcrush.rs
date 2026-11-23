use crate::audio::dsp::FrameProcessor;
use crate::audio::presets::lerp_by_step;
use anyhow::bail;
use serde::{Deserialize, Serialize};

const BITCRUSH_BITS: (u8, u8) = (1, 16);

#[derive(Deserialize, Serialize, Clone, Copy, Debug)]
pub struct Bitcrush {
    pub bits: u8,
}

lerp_by_step!(Option<Bitcrush>);

impl Bitcrush {
    pub fn check_limits(&self) -> anyhow::Result<()> {
        let (min, max) = BITCRUSH_BITS;
        if !(min..=max).contains(&self.bits) {
            bail!(
                "bitcrush.bits must be between {min} and {max}, got {}",
                self.bits
            );
        }
        Ok(())
    }
}

impl FrameProcessor for Bitcrush {
    fn process_frame(&mut self, frame: [f32; 2]) -> [f32; 2] {
        let step_size = 1.0 / 2.0_f32.powi(self.bits as i32 - 1);
        frame.map(|sample| (sample / step_size).round() * step_size)
    }
}
