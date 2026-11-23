use crate::audio::dsp::{EntryProcessor, FrameProcessor};
use crate::audio::helpers::perceptual_level_to_amplitude;
use crate::audio::presets::{ListEntry, lerp_by_step, lerp_struct};
use rand::{RngExt, rngs::StdRng};
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq, Eq, strum::Display)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum NoiseKind {
    White,
    Pink,
    Brown,
}

lerp_by_step!(NoiseKind);

lerp_struct! {
    #[derive(Deserialize, Serialize, Clone, Copy, Debug, garde::Validate)]
    pub struct Noise {
        #[garde(skip)]
        pub kind: NoiseKind,
        /// Noise levels (0.0-2.0) use perceptual (dB-based) scaling:
        /// - 0.0 = silence
        /// - 0.5 = moderate (-30dB)
        /// - 1.0 = reference level (0dB)
        /// - 2.0 = boosted (+18dB)
        #[garde(range(min = 0.0, max = 2.0))]
        pub level: f32,
    }
}

impl ListEntry for Noise {
    type Kind = NoiseKind;

    fn kind(&self) -> NoiseKind {
        self.kind
    }

    fn neutral(kind: NoiseKind) -> Self {
        Self { kind, level: 0.0 }
    }
}

enum NoiseSource {
    White(WhiteNoiseState),
    Pink(PinkNoiseState),
    Brown(BrownNoiseState),
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

struct BrownNoiseState {
    last_value: f32,
    rng: StdRng,
}

impl NoiseSource {
    fn new(kind: NoiseKind) -> Self {
        let rng = rand::make_rng();
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

    fn kind(&self) -> NoiseKind {
        match self {
            Self::White(_) => NoiseKind::White,
            Self::Pink(_) => NoiseKind::Pink,
            Self::Brown(_) => NoiseKind::Brown,
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

pub struct NoiseProcessor {
    source: NoiseSource,
    amplitude: f32,
}

impl EntryProcessor for NoiseProcessor {
    type Entry = Noise;

    fn new(_sample_rate: u32, noise: &Noise) -> Self {
        Self {
            source: NoiseSource::new(noise.kind),
            amplitude: perceptual_level_to_amplitude(noise.level),
        }
    }

    fn kind(&self) -> NoiseKind {
        self.source.kind()
    }

    fn update(&mut self, noise: &Noise) {
        self.amplitude = perceptual_level_to_amplitude(noise.level);
    }
}

impl FrameProcessor for NoiseProcessor {
    fn process_frame(&mut self, frame: [f32; 2]) -> [f32; 2] {
        let sample = self.source.next() * self.amplitude;
        [frame[0] + sample, frame[1] + sample]
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
