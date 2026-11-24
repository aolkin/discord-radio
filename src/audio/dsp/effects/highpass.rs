use crate::audio::dsp::filter::StereoFilter;
use crate::audio::dsp::{Effect, FrameProcessor};

#[derive(Clone, Copy, Debug)]
pub struct HighpassParams {
    pub frequency: f32,
}

pub struct Highpass {
    filter: StereoFilter,
}

impl Effect for Highpass {
    type Params = HighpassParams;

    fn new(sample_rate: u32, params: &HighpassParams) -> Self {
        Self {
            filter: StereoFilter::new(biquad::Type::HighPass, sample_rate, params.frequency),
        }
    }

    fn update(&mut self, params: &HighpassParams) {
        self.filter.set_frequency(params.frequency);
    }
}

impl FrameProcessor for Highpass {
    fn process_frame(&mut self, frame: [f32; 2]) -> [f32; 2] {
        self.filter.process_frame(frame)
    }
}
