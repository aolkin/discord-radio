use crate::audio::dsp::{Effect, FrameProcessor};

#[derive(Clone, Copy, Debug)]
pub struct BitcrushParams {
    pub bits: u8,
}

pub struct Bitcrush {
    params: BitcrushParams,
}

impl Effect for Bitcrush {
    type Params = BitcrushParams;

    fn new(_sample_rate: u32, params: &BitcrushParams) -> Self {
        Self { params: *params }
    }

    fn update(&mut self, params: &BitcrushParams) {
        self.params = *params;
    }
}

impl FrameProcessor for Bitcrush {
    fn process_frame(&mut self, frame: [f32; 2]) -> [f32; 2] {
        let step_size = 1.0 / 2.0_f32.powi(self.params.bits as i32 - 1);
        frame.map(|sample| (sample / step_size).round() * step_size)
    }
}
