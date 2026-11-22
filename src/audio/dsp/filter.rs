use crate::audio::dsp::{FrameProcessor, LayerProcessor};
use crate::audio::presets::{Filter, FilterKind};
use biquad::{Biquad, Coefficients, DirectForm1, ToHertz};

pub struct FilterProcessor {
    kind: FilterKind,
    sample_rate: u32,
    left: DirectForm1<f32>,
    right: DirectForm1<f32>,
}

fn coefficients(sample_rate: u32, filter: &Filter) -> Coefficients<f32> {
    let kind = match filter.kind {
        FilterKind::HighPass => biquad::Type::HighPass,
        FilterKind::LowPass => biquad::Type::LowPass,
    };
    Coefficients::<f32>::from_params(
        kind,
        sample_rate.hz(),
        filter.frequency.hz(),
        biquad::Q_BUTTERWORTH_F32,
    )
    .unwrap()
}

impl LayerProcessor for FilterProcessor {
    type Layer = Filter;

    fn new(sample_rate: u32, filter: &Filter) -> Self {
        let coefficients = coefficients(sample_rate, filter);
        Self {
            kind: filter.kind,
            sample_rate,
            left: DirectForm1::<f32>::new(coefficients),
            right: DirectForm1::<f32>::new(coefficients),
        }
    }

    fn kind(&self) -> FilterKind {
        self.kind
    }

    // Update coefficients without resetting filter state to avoid transients
    fn update(&mut self, filter: &Filter) {
        let coefficients = coefficients(self.sample_rate, filter);
        self.left.update_coefficients(coefficients);
        self.right.update_coefficients(coefficients);
    }
}

impl FrameProcessor for FilterProcessor {
    fn process_frame(&mut self, frame: [f32; 2]) -> [f32; 2] {
        [self.left.run(frame[0]), self.right.run(frame[1])]
    }
}
