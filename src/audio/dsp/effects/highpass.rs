use crate::audio::dsp::filter::{FilterParams, StereoFilter};
use crate::audio::dsp::{Effect, FrameProcessor};

pub struct Highpass {
    filter: StereoFilter,
}

impl Effect for Highpass {
    type Params = FilterParams;

    fn new(sample_rate: u32, params: &FilterParams) -> Self {
        Self {
            filter: StereoFilter::new(biquad::Type::HighPass, sample_rate, params.frequency),
        }
    }

    fn update(&mut self, params: &FilterParams) {
        self.filter.set_frequency(params.frequency);
    }
}

impl FrameProcessor for Highpass {
    fn process_frame(&mut self, frame: [f32; 2]) -> [f32; 2] {
        self.filter.process_frame(frame)
    }
}
