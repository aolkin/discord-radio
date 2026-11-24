use biquad::{Biquad, Coefficients, DirectForm1, ToHertz, Type};

#[derive(Clone, Copy, Debug)]
pub struct FilterParams {
    pub frequency: f32,
}

pub struct StereoFilter {
    response: Type<f32>,
    sample_rate: u32,
    left: DirectForm1<f32>,
    right: DirectForm1<f32>,
}

impl StereoFilter {
    pub fn new(response: Type<f32>, sample_rate: u32, frequency: f32) -> Self {
        let coefficients = coefficients(response, sample_rate, frequency);
        Self {
            response,
            sample_rate,
            left: DirectForm1::<f32>::new(coefficients),
            right: DirectForm1::<f32>::new(coefficients),
        }
    }

    pub fn set_frequency(&mut self, frequency: f32) {
        let coefficients = coefficients(self.response, self.sample_rate, frequency);
        self.left.update_coefficients(coefficients);
        self.right.update_coefficients(coefficients);
    }

    pub fn process_frame(&mut self, frame: [f32; 2]) -> [f32; 2] {
        [self.left.run(frame[0]), self.right.run(frame[1])]
    }
}

fn coefficients(response: Type<f32>, sample_rate: u32, frequency: f32) -> Coefficients<f32> {
    Coefficients::<f32>::from_params(
        response,
        sample_rate.hz(),
        frequency.hz(),
        biquad::Q_BUTTERWORTH_F32,
    )
    .unwrap()
}
