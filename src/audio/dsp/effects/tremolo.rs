use crate::audio::dsp::FrameProcessor;
use crate::audio::dsp::lfo::LFO;
use crate::audio::presets::{UNIT, check, lerp_struct};
use serde::{Deserialize, Serialize};

const TREMOLO_RATE_HZ: (f32, f32) = (0.0, 20.0);

lerp_struct! {
    #[derive(Deserialize, Serialize, Clone, Copy, Debug)]
    pub struct Tremolo {
        pub depth: f32,
        pub rate: f32,
        pub jitter: f32,
    }
}

impl Tremolo {
    pub fn check_limits(&self) -> anyhow::Result<()> {
        check("tremolo.depth", self.depth, UNIT)?;
        check("tremolo.rate", self.rate, TREMOLO_RATE_HZ)?;
        check("tremolo.jitter", self.jitter, UNIT)
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
