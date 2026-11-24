use crate::audio::dsp::lfo::LFO;
use crate::audio::dsp::{Effect, FrameProcessor};
use crate::audio::presets::lerp_struct;
use serde::{Deserialize, Serialize};

lerp_struct! {
    #[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq, garde::Validate)]
    pub struct TremoloParams {
        #[garde(range(min = 0.0, max = 1.0))]
        pub depth: f32,
        #[garde(range(min = 0.0, max = 20.0))]
        pub rate: f32,
        #[garde(range(min = 0.0, max = 1.0))]
        pub jitter: f32,
    }
}

pub struct Tremolo {
    lfo: LFO,
    depth: f32,
}

impl Effect for Tremolo {
    type Params = TremoloParams;

    fn new(sample_rate: u32, params: &TremoloParams) -> Self {
        Self {
            lfo: LFO::new(sample_rate, params.rate, params.jitter),
            depth: params.depth,
        }
    }

    fn update(&mut self, params: &TremoloParams) {
        self.depth = params.depth;
        self.lfo.set_frequency(params.rate);
        self.lfo.set_amplitude_jitter(params.jitter);
    }
}

impl FrameProcessor for Tremolo {
    fn process_frame(&mut self, frame: [f32; 2]) -> [f32; 2] {
        let lfo = self.lfo.next();
        let gain = 1.0 - self.depth + (lfo * 0.5 + 0.5) * self.depth;
        [frame[0] * gain, frame[1] * gain]
    }
}
