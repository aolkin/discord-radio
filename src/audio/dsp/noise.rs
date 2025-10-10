use crate::audio::profiles::NoiseType;
use dasp_signal::{self as signal, Signal};
use rand::{rngs::ThreadRng, Rng};

pub struct NoiseGenerator {
    noise_type: NoiseType,
    white_signal: Box<dyn Signal<Frame = f64> + Send>,
    pink_state: PinkNoiseState,
}

struct PinkNoiseState {
    state: [f32; 7],
    counter: u32,
    rng: ThreadRng,
}

impl NoiseGenerator {
    pub fn new(noise_type: NoiseType) -> Self {
        Self {
            noise_type,
            white_signal: Box::new(signal::noise(0)),
            pink_state: PinkNoiseState {
                state: [0.0; 7],
                counter: 0,
                rng: rand::rng(),
            },
        }
    }

    pub fn set_type(&mut self, noise_type: NoiseType) {
        self.noise_type = noise_type;
    }

    pub fn next_frame(&mut self) -> [f32; 2] {
        match self.noise_type {
            NoiseType::White => {
                let sample = self.white_signal.next() as f32;
                [sample, sample]
            }
            NoiseType::Pink => {
                let sample = self.pink_state.next();
                [sample, sample]
            }
        }
    }
}

impl PinkNoiseState {
    fn next(&mut self) -> f32 {
        let mut pink = 0.0;

        for i in 0..7 {
            if self.counter % (1 << i) == 0 {
                self.state[i] = self.rng.random::<f32>() * 2.0 - 1.0;
            }
            pink += self.state[i];
        }

        self.counter = self.counter.wrapping_add(1);

        pink / 7.0
    }
}
