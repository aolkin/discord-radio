use crate::audio::dsp::effects::bitcrush::Bitcrush;
use crate::audio::dsp::effects::clip::Clip;
use crate::audio::dsp::effects::dropout::Dropout;
use crate::audio::dsp::effects::highpass::Highpass;
use crate::audio::dsp::effects::lowpass::Lowpass;
use crate::audio::dsp::effects::noise::Noise;
use crate::audio::dsp::effects::tremolo::Tremolo;
use crate::audio::dsp::{Effect, FrameProcessor, ambassador_impl_FrameProcessor};
use crate::audio::presets::{EffectEntry, EffectParams, SignalParameters};
use ambassador::Delegate;

#[derive(Delegate)]
#[delegate(FrameProcessor)]
pub enum EffectProcessor {
    Noise(Noise),
    Highpass(Highpass),
    Lowpass(Lowpass),
    Tremolo(Tremolo),
    Clip(Clip),
    Bitcrush(Bitcrush),
    Dropout(Dropout),
}

impl EffectProcessor {
    fn new(sample_rate: u32, params: &EffectParams) -> Self {
        match params {
            EffectParams::Noise(p) => Self::Noise(Noise::new(sample_rate, p)),
            EffectParams::Highpass(p) => Self::Highpass(Highpass::new(sample_rate, p)),
            EffectParams::Lowpass(p) => Self::Lowpass(Lowpass::new(sample_rate, p)),
            EffectParams::Tremolo(p) => Self::Tremolo(Tremolo::new(sample_rate, p)),
            EffectParams::Clip(p) => Self::Clip(Clip::new(sample_rate, p)),
            EffectParams::Bitcrush(p) => Self::Bitcrush(Bitcrush::new(sample_rate, p)),
            EffectParams::Dropout(p) => Self::Dropout(Dropout::new(sample_rate, p)),
        }
    }

    fn try_update(&mut self, params: &EffectParams) -> bool {
        match (self, params) {
            (Self::Noise(e), EffectParams::Noise(p)) if e.color == p.color => e.update(p),
            (Self::Highpass(e), EffectParams::Highpass(p)) => e.update(p),
            (Self::Lowpass(e), EffectParams::Lowpass(p)) => e.update(p),
            (Self::Tremolo(e), EffectParams::Tremolo(p)) => e.update(p),
            (Self::Clip(e), EffectParams::Clip(p)) => e.update(p),
            (Self::Bitcrush(e), EffectParams::Bitcrush(p)) => e.update(p),
            (Self::Dropout(e), EffectParams::Dropout(p)) => e.update(p),
            _ => return false,
        }
        true
    }
}

pub struct RadioEffectChain {
    sample_rate: u32,
    effects: Vec<(String, EffectProcessor)>,
}

impl RadioEffectChain {
    pub fn new(sample_rate: u32, preset: &SignalParameters) -> Self {
        let mut chain = Self {
            sample_rate,
            effects: Vec::new(),
        };
        chain.update_preset(preset);
        chain
    }

    pub fn update_preset(&mut self, preset: &SignalParameters) {
        let mut previous = std::mem::take(&mut self.effects);
        self.effects = preset
            .effects
            .iter()
            .map(|EffectEntry { name, effect }| {
                let reused = previous
                    .iter_mut()
                    .position(|(old_name, processor)| {
                        old_name == name && processor.try_update(effect)
                    })
                    .map(|index| previous.remove(index).1);
                let processor =
                    reused.unwrap_or_else(|| EffectProcessor::new(self.sample_rate, effect));
                (name.clone(), processor)
            })
            .collect();
    }
}

impl FrameProcessor for RadioEffectChain {
    fn process_frame(&mut self, frame: [f32; 2]) -> [f32; 2] {
        self.effects
            .iter_mut()
            .fold(frame, |frame, (_, processor)| {
                processor.process_frame(frame)
            })
    }
}
