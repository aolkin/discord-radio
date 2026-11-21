use crate::audio::presets::NoiseKind;
use rand::{RngExt, SeedableRng, rngs::StdRng};

pub enum NoiseSource {
    White(WhiteNoiseState),
    Pink(PinkNoiseState),
    Brown(BrownNoiseState),
}

pub struct WhiteNoiseState {
    rng: StdRng,
}

impl WhiteNoiseState {
    fn next(&mut self) -> f32 {
        self.rng.random::<f32>() * 2.0 - 1.0
    }
}

pub struct PinkNoiseState {
    state: [f32; 7],
    counter: u32,
    rng: StdRng,
}

pub struct BrownNoiseState {
    last_value: f32,
    rng: StdRng,
}

fn seeded_rng() -> StdRng {
    use rand::Rng;
    let mut seed = [0u8; 32];
    rand::rng().fill_bytes(&mut seed);
    StdRng::from_seed(seed)
}

impl NoiseSource {
    pub fn new(kind: NoiseKind) -> Self {
        let rng = seeded_rng();
        match kind {
            NoiseKind::White => Self::White(WhiteNoiseState { rng }),
            NoiseKind::Pink => Self::Pink(PinkNoiseState {
                state: [0.0; 7],
                counter: 0,
                rng,
            }),
            NoiseKind::Brown => Self::Brown(BrownNoiseState {
                last_value: 0.0,
                rng,
            }),
        }
    }

    pub fn next(&mut self) -> f32 {
        match self {
            Self::White(state) => state.next(),
            Self::Pink(state) => state.next(),
            Self::Brown(state) => state.next(),
        }
    }
}

impl PinkNoiseState {
    fn next(&mut self) -> f32 {
        let mut pink = 0.0;

        for i in 0..7 {
            if self.counter.is_multiple_of(1 << i) {
                self.state[i] = self.rng.random::<f32>() * 2.0 - 1.0;
            }
            pink += self.state[i];
        }

        self.counter = self.counter.wrapping_add(1);

        pink / 7.0
    }
}

impl BrownNoiseState {
    fn next(&mut self) -> f32 {
        let white = self.rng.random::<f32>() * 2.0 - 1.0;
        self.last_value = (self.last_value + white * 0.02).clamp(-1.0, 1.0);
        self.last_value
    }
}
