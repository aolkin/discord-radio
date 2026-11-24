use crate::audio::dsp::filter::StereoFilter;
use crate::audio::dsp::{Effect, FrameProcessor};

#[derive(Clone, Copy, Debug)]
pub struct LowpassParams {
    pub frequency: f32,
}

pub struct Lowpass {
    filter: StereoFilter,
}

impl Effect for Lowpass {
    type Params = LowpassParams;

    fn new(sample_rate: u32, params: &LowpassParams) -> Self {
        Self {
            filter: StereoFilter::new(biquad::Type::LowPass, sample_rate, params.frequency),
        }
    }

    fn update(&mut self, params: &LowpassParams) {
        self.filter.set_frequency(params.frequency);
    }
}

impl FrameProcessor for Lowpass {
    fn process_frame(&mut self, frame: [f32; 2]) -> [f32; 2] {
        self.filter.process_frame(frame)
    }
}
