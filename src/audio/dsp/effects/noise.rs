use crate::audio::dsp::{Effect, FrameProcessor};
use crate::audio::helpers::perceptual_level_to_amplitude;
use rand::{RngExt, rngs::StdRng};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoiseColor {
    White,
    Pink,
    Brown,
}

#[derive(Clone, Copy, Debug)]
pub struct NoiseParams {
    pub color: NoiseColor,
    pub level: f32,
}

pub struct Noise {
    color: NoiseColor,
    source: NoiseSource,
    amplitude: f32,
}

impl Effect for Noise {
    type Params = NoiseParams;

    fn new(_sample_rate: u32, params: &NoiseParams) -> Self {
        Self {
            color: params.color,
            source: NoiseSource::new(params.color),
            amplitude: perceptual_level_to_amplitude(params.level),
        }
    }

    fn update(&mut self, params: &NoiseParams) {
        if params.color != self.color {
            self.color = params.color;
            self.source = NoiseSource::new(params.color);
        }
        self.amplitude = perceptual_level_to_amplitude(params.level);
    }
}

impl FrameProcessor for Noise {
    fn process_frame(&mut self, frame: [f32; 2]) -> [f32; 2] {
        let sample = self.source.next() * self.amplitude;
        [frame[0] + sample, frame[1] + sample]
    }
}

enum NoiseSource {
    White(WhiteNoiseState),
    Pink(PinkNoiseState),
    Brown(BrownNoiseState),
}

impl NoiseSource {
    fn new(color: NoiseColor) -> Self {
        let rng = rand::make_rng();
        match color {
            NoiseColor::White => Self::White(WhiteNoiseState { rng }),
            NoiseColor::Pink => Self::Pink(PinkNoiseState {
                state: [0.0; 7],
                counter: 0,
                rng,
            }),
            NoiseColor::Brown => Self::Brown(BrownNoiseState {
                last_value: 0.0,
                rng,
            }),
        }
    }

    fn next(&mut self) -> f32 {
        match self {
            Self::White(state) => state.next(),
            Self::Pink(state) => state.next(),
            Self::Brown(state) => state.next(),
        }
    }
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

struct BrownNoiseState {
    last_value: f32,
    rng: StdRng,
}

impl BrownNoiseState {
    fn next(&mut self) -> f32 {
        let white = self.rng.random::<f32>() * 2.0 - 1.0;
        self.last_value = (self.last_value + white * 0.02).clamp(-1.0, 1.0);
        self.last_value
    }
}
