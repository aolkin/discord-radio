use crate::audio::dsp::modulation::{Bitcrusher, DropoutGenerator, LFO};
use crate::audio::dsp::noise::NoiseSource;
use crate::audio::helpers::perceptual_level_to_amplitude;
use crate::audio::presets::{Filter, FilterKind, Noise, NoiseKind, SignalParameters};
use atomic_float::AtomicF32;
use biquad::{Biquad, Coefficients, DirectForm1, ToHertz};
use std::sync::Arc;
use std::sync::atomic::Ordering;

struct NoiseLayer {
    kind: NoiseKind,
    source: NoiseSource,
    amplitude: f32,
}

struct FilterStage {
    kind: FilterKind,
    left: DirectForm1<f32>,
    right: DirectForm1<f32>,
}

pub struct RadioEffectChain {
    noise: Vec<NoiseLayer>,

    filters: Vec<FilterStage>,

    tremolo_lfo: LFO,
    depth: Arc<AtomicF32>,

    pregain: Arc<AtomicF32>,
    threshold: Arc<AtomicF32>,

    bitcrusher: Option<Bitcrusher>,

    dropout_gen: DropoutGenerator,

    sample_rate: u32,
}

impl RadioEffectChain {
    pub fn new(sample_rate: u32, preset: &SignalParameters) -> Self {
        let mut chain = Self {
            noise: Vec::new(),
            filters: Vec::new(),

            tremolo_lfo: LFO::new(sample_rate, preset.tremolo.rate, preset.tremolo.jitter),
            depth: Arc::new(AtomicF32::new(preset.tremolo.depth)),

            pregain: Arc::new(AtomicF32::new(preset.clip.pregain)),
            threshold: Arc::new(AtomicF32::new(preset.clip.threshold)),

            bitcrusher: preset
                .bitcrush
                .map(|bitcrush| Bitcrusher::new(bitcrush.bits)),

            dropout_gen: DropoutGenerator::new(&preset.dropout, sample_rate),

            sample_rate,
        };
        chain.update_noise(&preset.noise);
        chain.update_filters(&preset.filters);
        chain
    }

    pub fn update_preset(&mut self, preset: &SignalParameters) {
        self.update_noise(&preset.noise);
        self.update_filters(&preset.filters);
        self.depth.store(preset.tremolo.depth, Ordering::Relaxed);
        self.tremolo_lfo
            .set_frequency(self.sample_rate, preset.tremolo.rate);
        self.tremolo_lfo.set_amplitude_jitter(preset.tremolo.jitter);
        self.dropout_gen.set_params(&preset.dropout);
        self.pregain.store(preset.clip.pregain, Ordering::Relaxed);
        self.threshold
            .store(preset.clip.threshold, Ordering::Relaxed);

        if let Some(bitcrush) = preset.bitcrush {
            if let Some(ref mut bc) = self.bitcrusher {
                bc.set_depth(bitcrush.bits);
            } else {
                self.bitcrusher = Some(Bitcrusher::new(bitcrush.bits));
            }
        } else {
            self.bitcrusher = None;
        }
    }

    fn update_noise(&mut self, noise: &[Noise]) {
        self.noise
            .retain(|layer| noise.iter().any(|params| params.kind == layer.kind));
        for params in noise {
            let amplitude = perceptual_level_to_amplitude(params.level);
            match self
                .noise
                .iter_mut()
                .find(|layer| layer.kind == params.kind)
            {
                Some(layer) => layer.amplitude = amplitude,
                None => self.noise.push(NoiseLayer {
                    kind: params.kind,
                    source: NoiseSource::new(params.kind),
                    amplitude,
                }),
            }
        }
    }

    fn update_filters(&mut self, filters: &[Filter]) {
        self.filters
            .retain(|stage| filters.iter().any(|filter| filter.kind == stage.kind));
        for filter in filters {
            let coefficients = filter_coefficients(self.sample_rate, filter);
            match self
                .filters
                .iter_mut()
                .find(|stage| stage.kind == filter.kind)
            {
                // Update coefficients without resetting filter state to avoid transients
                Some(stage) => {
                    stage.left.update_coefficients(coefficients);
                    stage.right.update_coefficients(coefficients);
                }
                None => self.filters.push(FilterStage {
                    kind: filter.kind,
                    left: DirectForm1::<f32>::new(coefficients),
                    right: DirectForm1::<f32>::new(coefficients),
                }),
            }
        }
    }

    pub fn process_frame(&mut self, input: [f32; 2]) -> [f32; 2] {
        let mut frame = input;

        // 1. Noise mixing - Add static/interference
        for layer in &mut self.noise {
            let sample = layer.source.next() * layer.amplitude;
            frame[0] += sample;
            frame[1] += sample;
        }

        // 2. Filters - Simulate radio frequency response
        // e.g., 800Hz highpass + 4500Hz lowpass = "telephone" bandlimiting
        for stage in &mut self.filters {
            frame[0] = stage.left.run(frame[0]);
            frame[1] = stage.right.run(frame[1]);
        }

        // 3. Tremolo - Simulate signal strength fluctuation
        // LFO modulates amplitude to create wavering effect
        let lfo = self.tremolo_lfo.next();
        let depth = self.depth.load(Ordering::Relaxed);
        let tremolo_gain = 1.0 - depth + (lfo * 0.5 + 0.5) * depth;
        frame[0] *= tremolo_gain;
        frame[1] *= tremolo_gain;

        // 4. Soft clipping - Simulate overdriven/distorted signal
        // Apply pregain, then clip to threshold to create distortion
        let pregain = self.pregain.load(Ordering::Relaxed);
        let threshold = self.threshold.load(Ordering::Relaxed);
        frame[0] = soft_clip(frame[0] * pregain, threshold);
        frame[1] = soft_clip(frame[1] * pregain, threshold);

        // 5. Bitcrushing - Simulate low-quality digital encoding
        // Reduces bit depth to create lo-fi digital artifacts
        if let Some(ref bitcrusher) = self.bitcrusher {
            frame = bitcrusher.process_frame(frame);
        }

        // 6. Dropouts - Simulate intermittent signal loss
        // Randomly mutes audio to simulate interference/connection issues
        if self.dropout_gen.should_dropout() {
            frame = [0.0, 0.0];
        }

        frame
    }
}

fn filter_coefficients(sample_rate: u32, filter: &Filter) -> Coefficients<f32> {
    let kind = match filter.kind {
        FilterKind::HighPass => biquad::Type::HighPass,
        FilterKind::LowPass => biquad::Type::LowPass,
    };
    Coefficients::<f32>::from_params(
        kind,
        sample_rate.hz(),
        filter.frequency.hz(),
        biquad::Q_BUTTERWORTH_F32,
    )
    .unwrap()
}

fn soft_clip(sample: f32, threshold: f32) -> f32 {
    if sample.abs() < threshold {
        sample
    } else {
        threshold * sample.signum() * (1.0 - (-sample.abs() / threshold).exp())
    }
}
