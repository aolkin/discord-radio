use crate::audio::profiles::NoiseType;
use rand::{rngs::StdRng, Rng, SeedableRng};

pub struct NoiseGenerator {
    noise_type: NoiseType,
    white_state: WhiteNoiseState,
    pink_state: PinkNoiseState,
}

struct WhiteNoiseState {
    rng: StdRng,
}

impl WhiteNoiseState {
    fn next(&mut self) -> f32 {
        self.rng.random::<f32>() * 2.0 - 1.0
    }
}

struct PinkNoiseState {
    state: [f32; 7],
    counter: u32,
    rng: StdRng,
}

impl NoiseGenerator {
    pub fn new(noise_type: NoiseType) -> Self {
        use rand::RngCore;
        let mut seed_rng = rand::rng();
        let mut seed1 = [0u8; 32];
        let mut seed2 = [0u8; 32];
        seed_rng.fill_bytes(&mut seed1);
        seed_rng.fill_bytes(&mut seed2);

        Self {
            noise_type,
            white_state: WhiteNoiseState {
                rng: StdRng::from_seed(seed1),
            },
            pink_state: PinkNoiseState {
                state: [0.0; 7],
                counter: 0,
                rng: StdRng::from_seed(seed2),
            },
        }
    }

    pub fn set_type(&mut self, noise_type: NoiseType) {
        self.noise_type = noise_type;
    }

    pub fn next_frame(&mut self) -> [f32; 2] {
        match self.noise_type {
            NoiseType::White => {
                let sample = self.white_state.next();
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
