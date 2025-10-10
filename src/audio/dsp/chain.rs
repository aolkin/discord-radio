use crate::audio::dsp::modulation::{Bitcrusher, DropoutGenerator, LFO, PitchShifter};
use crate::audio::dsp::noise::NoiseGenerator;
use crate::audio::profiles::SignalProfile;
use atomic_float::AtomicF32;
use biquad::{Biquad, Coefficients, DirectForm2Transposed, ToHertz};
use std::sync::atomic::Ordering;
use std::sync::Arc;

pub struct RadioEffectChain {
    bandpass_l: DirectForm2Transposed<f32>,
    bandpass_r: DirectForm2Transposed<f32>,

    noise_gen: NoiseGenerator,
    noise_mix: Arc<AtomicF32>,

    tremolo_lfo: LFO,
    tremolo_depth: Arc<AtomicF32>,

    pitch_shifter: Option<PitchShifter>,

    clip_threshold: Arc<AtomicF32>,

    bitcrusher: Option<Bitcrusher>,

    dropout_gen: DropoutGenerator,

    sample_rate: u32,
}

impl RadioEffectChain {
    pub fn new(sample_rate: u32, profile: &SignalProfile) -> Self {
        let center_freq = (profile.bandpass_low + profile.bandpass_high) / 2.0;
        let bandwidth = profile.bandpass_high - profile.bandpass_low;
        let q = center_freq / bandwidth;

        let bp_coeffs = Coefficients::<f32>::from_params(
            biquad::Type::BandPass,
            sample_rate.hz(),
            center_freq.hz(),
            q,
        )
        .unwrap();

        let pitch_shifter = profile.pitch_shift_cents.map(|cents| {
            PitchShifter::new(Arc::new(AtomicF32::new(cents)))
        });

        let bitcrusher = profile.bitcrush_bits.map(Bitcrusher::new);

        Self {
            bandpass_l: DirectForm2Transposed::<f32>::new(bp_coeffs),
            bandpass_r: DirectForm2Transposed::<f32>::new(bp_coeffs),

            noise_gen: NoiseGenerator::new(profile.noise_type.clone()),
            noise_mix: Arc::new(AtomicF32::new(profile.noise_level)),

            tremolo_lfo: LFO::new(sample_rate, profile.tremolo_rate),
            tremolo_depth: Arc::new(AtomicF32::new(profile.tremolo_depth)),

            pitch_shifter,

            clip_threshold: Arc::new(AtomicF32::new(profile.clip_threshold)),

            bitcrusher,

            dropout_gen: DropoutGenerator::new(
                profile.dropout_probability,
                profile.dropout_duration_ms,
                sample_rate,
            ),

            sample_rate,
        }
    }

    pub fn update_profile(&mut self, profile: &SignalProfile) {
        let center_freq = (profile.bandpass_low + profile.bandpass_high) / 2.0;
        let bandwidth = profile.bandpass_high - profile.bandpass_low;
        let q = center_freq / bandwidth;

        let bp_coeffs = Coefficients::<f32>::from_params(
            biquad::Type::BandPass,
            self.sample_rate.hz(),
            center_freq.hz(),
            q,
        )
        .unwrap();

        self.bandpass_l = DirectForm2Transposed::<f32>::new(bp_coeffs);
        self.bandpass_r = DirectForm2Transposed::<f32>::new(bp_coeffs);

        self.noise_gen.set_type(profile.noise_type.clone());
        self.noise_mix.store(profile.noise_level, Ordering::Relaxed);
        self.tremolo_depth.store(profile.tremolo_depth, Ordering::Relaxed);
        self.clip_threshold.store(profile.clip_threshold, Ordering::Relaxed);

        if let Some(bits) = profile.bitcrush_bits {
            if let Some(ref mut bc) = self.bitcrusher {
                bc.set_depth(bits);
            } else {
                self.bitcrusher = Some(Bitcrusher::new(bits));
            }
        } else {
            self.bitcrusher = None;
        }

        if let Some(cents) = profile.pitch_shift_cents {
            if let Some(ref mut ps) = self.pitch_shifter {
                ps.cents.store(cents, Ordering::Relaxed);
            } else {
                self.pitch_shifter = Some(PitchShifter::new(Arc::new(AtomicF32::new(cents))));
            }
        } else {
            self.pitch_shifter = None;
        }
    }

    pub fn process_frame(&mut self, input: [f32; 2]) -> [f32; 2] {
        let mut frame = input;

        frame[0] = self.bandpass_l.run(frame[0]);
        frame[1] = self.bandpass_r.run(frame[1]);

        let noise_frame = self.noise_gen.next_frame();
        let mix = self.noise_mix.load(Ordering::Relaxed);
        frame[0] = frame[0] * (1.0 - mix) + noise_frame[0] * mix;
        frame[1] = frame[1] * (1.0 - mix) + noise_frame[1] * mix;

        let lfo = self.tremolo_lfo.next();
        let depth = self.tremolo_depth.load(Ordering::Relaxed);
        let tremolo_gain = 1.0 - depth + (lfo * 0.5 + 0.5) * depth;
        frame[0] *= tremolo_gain;
        frame[1] *= tremolo_gain;

        if let Some(ref mut pitch_shifter) = self.pitch_shifter {
            frame = pitch_shifter.process(frame);
        }

        let threshold = self.clip_threshold.load(Ordering::Relaxed);
        frame[0] = soft_clip(frame[0], threshold);
        frame[1] = soft_clip(frame[1], threshold);

        if let Some(ref bitcrusher) = self.bitcrusher {
            frame = bitcrusher.process_frame(frame);
        }

        if self.dropout_gen.should_dropout() {
            frame = [0.0, 0.0];
        }

        frame
    }

    pub fn noise_mix(&self) -> Arc<AtomicF32> {
        self.noise_mix.clone()
    }

    pub fn tremolo_depth(&self) -> Arc<AtomicF32> {
        self.tremolo_depth.clone()
    }

    pub fn clip_threshold(&self) -> Arc<AtomicF32> {
        self.clip_threshold.clone()
    }
}

fn soft_clip(sample: f32, threshold: f32) -> f32 {
    if sample.abs() < threshold {
        sample
    } else {
        threshold * sample.signum() * (1.0 - (-sample.abs() / threshold).exp())
    }
}
