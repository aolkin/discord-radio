use crate::audio::dsp::effects::bitcrush::Bitcrush;
use crate::audio::dsp::effects::clip::Clip;
use crate::audio::dsp::effects::dropout::DropoutGenerator;
use crate::audio::dsp::effects::filter::FilterProcessor;
use crate::audio::dsp::effects::noise::NoiseProcessor;
use crate::audio::dsp::effects::tremolo::TremoloProcessor;
use crate::audio::dsp::{FrameProcessor, update_entries};
use crate::audio::presets::SignalParameters;

pub struct RadioEffectChain {
    sample_rate: u32,
    noise: Vec<NoiseProcessor>,
    filters: Vec<FilterProcessor>,
    tremolo: TremoloProcessor,
    clip: Clip,
    bitcrush: Option<Bitcrush>,
    dropout: DropoutGenerator,
}

impl RadioEffectChain {
    pub fn new(sample_rate: u32, preset: &SignalParameters) -> Self {
        let mut chain = Self {
            sample_rate,
            noise: Vec::new(),
            filters: Vec::new(),
            tremolo: TremoloProcessor::new(sample_rate, &preset.tremolo),
            clip: preset.clip,
            bitcrush: preset.bitcrush,
            dropout: DropoutGenerator::new(&preset.dropout, sample_rate),
        };
        chain.update_preset(preset);
        chain
    }

    pub fn update_preset(&mut self, preset: &SignalParameters) {
        update_entries(&mut self.noise, &preset.noise, self.sample_rate);
        update_entries(&mut self.filters, &preset.filters, self.sample_rate);
        self.tremolo.update(&preset.tremolo);
        self.clip = preset.clip;
        self.bitcrush = preset.bitcrush;
        self.dropout.update(&preset.dropout);
    }
}

impl FrameProcessor for RadioEffectChain {
    fn process_frame(&mut self, frame: [f32; 2]) -> [f32; 2] {
        let frame = self.noise.process_frame(frame);
        let frame = self.filters.process_frame(frame);
        let frame = self.tremolo.process_frame(frame);
        let frame = self.clip.process_frame(frame);
        let frame = self.bitcrush.process_frame(frame);
        self.dropout.process_frame(frame)
    }
}
