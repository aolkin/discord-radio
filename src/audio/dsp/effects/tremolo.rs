use crate::audio::dsp::FrameProcessor;
use crate::audio::dsp::lfo::LFO;
use crate::audio::presets::lerp_struct;
use serde::{Deserialize, Serialize};

lerp_struct! {
    #[derive(Deserialize, Serialize, Clone, Copy, Debug, garde::Validate)]
    pub struct Tremolo {
        #[garde(range(min = 0.0, max = 1.0))]
        pub depth: f32,
        #[garde(range(min = 0.0, max = 20.0))]
        pub rate: f32,
        #[garde(range(min = 0.0, max = 1.0))]
        pub jitter: f32,
    }
}

pub struct TremoloProcessor {
    lfo: LFO,
    depth: f32,
}

impl TremoloProcessor {
    pub fn new(sample_rate: u32, tremolo: &Tremolo) -> Self {
        Self {
            lfo: LFO::new(sample_rate, tremolo.rate, tremolo.jitter),
            depth: tremolo.depth,
        }
    }

    pub fn update(&mut self, tremolo: &Tremolo) {
        self.depth = tremolo.depth;
        self.lfo.set_frequency(tremolo.rate);
        self.lfo.set_amplitude_jitter(tremolo.jitter);
    }
}

impl FrameProcessor for TremoloProcessor {
    fn process_frame(&mut self, frame: [f32; 2]) -> [f32; 2] {
        let lfo = self.lfo.next();
        let gain = 1.0 - self.depth + (lfo * 0.5 + 0.5) * self.depth;
        [frame[0] * gain, frame[1] * gain]
    }
}
