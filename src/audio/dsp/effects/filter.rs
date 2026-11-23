use crate::audio::dsp::{EntryProcessor, FrameProcessor};
use crate::audio::presets::{ListEntry, lerp_by_step, lerp_struct};
use biquad::{Biquad, Coefficients, DirectForm1, ToHertz};
use serde::{Deserialize, Serialize};

const FILTER_HZ: (f32, f32) = (20.0, 20000.0);

#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq, Eq, strum::Display)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum FilterKind {
    HighPass,
    LowPass,
}

lerp_by_step!(FilterKind);

lerp_struct! {
    #[derive(Deserialize, Serialize, Clone, Copy, Debug, garde::Validate)]
    pub struct Filter {
        #[garde(skip)]
        pub kind: FilterKind,
        #[garde(range(min = FILTER_HZ.0, max = FILTER_HZ.1))]
        pub frequency: f32,
    }
}

impl ListEntry for Filter {
    type Kind = FilterKind;

    fn kind(&self) -> FilterKind {
        self.kind
    }

    fn neutral(kind: FilterKind) -> Self {
        let frequency = match kind {
            FilterKind::HighPass => FILTER_HZ.0,
            FilterKind::LowPass => FILTER_HZ.1,
        };
        Self { kind, frequency }
    }
}

pub fn highpass_below_lowpass(filters: &[Filter], _: &()) -> garde::Result {
    let frequencies = |kind| {
        filters
            .iter()
            .filter(move |filter| filter.kind == kind)
            .map(|filter| filter.frequency)
    };
    let lowest_lowpass = frequencies(FilterKind::LowPass).fold(f32::INFINITY, f32::min);
    if frequencies(FilterKind::HighPass).any(|highpass| highpass >= lowest_lowpass) {
        return Err(garde::Error::new(
            "every highpass frequency must be below every lowpass frequency",
        ));
    }
    Ok(())
}

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

impl EntryProcessor for FilterProcessor {
    type Entry = Filter;

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
