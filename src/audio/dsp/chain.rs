use crate::audio::dsp::effects::bitcrush::{Bitcrush, BitcrushParams};
use crate::audio::dsp::effects::clip::{Clip, ClipParams};
use crate::audio::dsp::effects::dropout::{Dropout, DropoutParams};
use crate::audio::dsp::effects::highpass::{Highpass, HighpassParams};
use crate::audio::dsp::effects::lowpass::{Lowpass, LowpassParams};
use crate::audio::dsp::effects::noise::{Noise, NoiseColor, NoiseParams};
use crate::audio::dsp::effects::tremolo::{Tremolo, TremoloParams};
use crate::audio::dsp::{Effect, FrameProcessor};
use crate::audio::presets::SignalParameters;

pub struct RadioEffectChain {
    sample_rate: u32,
    white_noise: Noise,
    pink_noise: Noise,
    brown_noise: Noise,
    highpass: Highpass,
    lowpass: Lowpass,
    tremolo: Tremolo,
    clip: Clip,
    bitcrush: Option<Bitcrush>,
    dropout: Dropout,
}

impl RadioEffectChain {
    pub fn new(sample_rate: u32, preset: &SignalParameters) -> Self {
        Self {
            sample_rate,
            white_noise: Noise::new(sample_rate, &noise(NoiseColor::White, preset)),
            pink_noise: Noise::new(sample_rate, &noise(NoiseColor::Pink, preset)),
            brown_noise: Noise::new(sample_rate, &noise(NoiseColor::Brown, preset)),
            highpass: Highpass::new(sample_rate, &highpass(preset)),
            lowpass: Lowpass::new(sample_rate, &lowpass(preset)),
            tremolo: Tremolo::new(sample_rate, &tremolo(preset)),
            clip: Clip::new(sample_rate, &clip(preset)),
            bitcrush: bitcrush(preset).map(|params| Bitcrush::new(sample_rate, &params)),
            dropout: Dropout::new(sample_rate, &dropout(preset)),
        }
    }

    pub fn update_preset(&mut self, preset: &SignalParameters) {
        self.white_noise.update(&noise(NoiseColor::White, preset));
        self.pink_noise.update(&noise(NoiseColor::Pink, preset));
        self.brown_noise.update(&noise(NoiseColor::Brown, preset));
        self.highpass.update(&highpass(preset));
        self.lowpass.update(&lowpass(preset));
        self.tremolo.update(&tremolo(preset));
        self.clip.update(&clip(preset));
        match (&mut self.bitcrush, bitcrush(preset)) {
            (Some(processor), Some(params)) => processor.update(&params),
            (processor, params) => {
                *processor = params.map(|params| Bitcrush::new(self.sample_rate, &params))
            }
        }
        self.dropout.update(&dropout(preset));
    }
}

impl FrameProcessor for RadioEffectChain {
    fn process_frame(&mut self, frame: [f32; 2]) -> [f32; 2] {
        let frame = self.white_noise.process_frame(frame);
        let frame = self.pink_noise.process_frame(frame);
        let frame = self.brown_noise.process_frame(frame);
        let frame = self.highpass.process_frame(frame);
        let frame = self.lowpass.process_frame(frame);
        let frame = self.tremolo.process_frame(frame);
        let mut frame = self.clip.process_frame(frame);
        if let Some(bitcrush) = &mut self.bitcrush {
            frame = bitcrush.process_frame(frame);
        }
        self.dropout.process_frame(frame)
    }
}

fn noise(color: NoiseColor, preset: &SignalParameters) -> NoiseParams {
    let level = match color {
        NoiseColor::White => preset.white_noise_level,
        NoiseColor::Pink => preset.pink_noise_level,
        NoiseColor::Brown => preset.brown_noise_level,
    };
    NoiseParams { color, level }
}

fn highpass(preset: &SignalParameters) -> HighpassParams {
    HighpassParams {
        frequency: preset.bandpass_low,
    }
}

fn lowpass(preset: &SignalParameters) -> LowpassParams {
    LowpassParams {
        frequency: preset.bandpass_high,
    }
}

fn tremolo(preset: &SignalParameters) -> TremoloParams {
    TremoloParams {
        depth: preset.tremolo_depth,
        rate: preset.tremolo_rate,
        jitter: preset.tremolo_jitter,
    }
}

fn clip(preset: &SignalParameters) -> ClipParams {
    ClipParams {
        pregain: preset.clip_pregain,
        threshold: preset.clip_threshold,
    }
}

fn bitcrush(preset: &SignalParameters) -> Option<BitcrushParams> {
    preset.bitcrush_bits.map(|bits| BitcrushParams { bits })
}

fn dropout(preset: &SignalParameters) -> DropoutParams {
    let (min_ms, max_ms) = preset.dropout_duration_ms;
    DropoutParams {
        probability: preset.dropout_probability,
        min_ms,
        max_ms,
    }
}
