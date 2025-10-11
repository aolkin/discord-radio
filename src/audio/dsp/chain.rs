use crate::audio::dsp::modulation::{Bitcrusher, DropoutGenerator, LFO};
use crate::audio::dsp::noise::NoiseGenerator;
use crate::audio::profiles::SignalProfile;
use atomic_float::AtomicF32;
use biquad::{Biquad, Coefficients, DirectForm1, ToHertz};
use std::sync::Arc;
use std::sync::atomic::Ordering;

pub struct RadioEffectChain {
    highpass_l: DirectForm1<f32>,
    highpass_r: DirectForm1<f32>,
    lowpass_l: DirectForm1<f32>,
    lowpass_r: DirectForm1<f32>,

    noise_gen: NoiseGenerator,
    white_noise_level: Arc<AtomicF32>,
    pink_noise_level: Arc<AtomicF32>,

    tremolo_lfo: LFO,
    tremolo_depth: Arc<AtomicF32>,

    clip_pregain: Arc<AtomicF32>,
    clip_threshold: Arc<AtomicF32>,

    bitcrusher: Option<Bitcrusher>,

    dropout_gen: DropoutGenerator,

    sample_rate: u32,

    bypass: bool,
}

impl RadioEffectChain {
    pub fn new(sample_rate: u32, profile: &SignalProfile) -> Self {
        // Use separate highpass and lowpass filters instead of bandpass
        // This gives true pass/reject behavior at frequency boundaries
        let hp_coeffs = Coefficients::<f32>::from_params(
            biquad::Type::HighPass,
            sample_rate.hz(),
            profile.bandpass_low.hz(),
            biquad::Q_BUTTERWORTH_F32,
        )
        .unwrap();

        let lp_coeffs = Coefficients::<f32>::from_params(
            biquad::Type::LowPass,
            sample_rate.hz(),
            profile.bandpass_high.hz(),
            biquad::Q_BUTTERWORTH_F32,
        )
        .unwrap();

        let bitcrusher = profile.bitcrush_bits.map(Bitcrusher::new);

        Self {
            highpass_l: DirectForm1::<f32>::new(hp_coeffs),
            highpass_r: DirectForm1::<f32>::new(hp_coeffs),
            lowpass_l: DirectForm1::<f32>::new(lp_coeffs),
            lowpass_r: DirectForm1::<f32>::new(lp_coeffs),

            noise_gen: NoiseGenerator::new(),
            white_noise_level: Arc::new(AtomicF32::new(profile.white_noise_level)),
            pink_noise_level: Arc::new(AtomicF32::new(profile.pink_noise_level)),

            tremolo_lfo: LFO::new(sample_rate, profile.tremolo_rate),
            tremolo_depth: Arc::new(AtomicF32::new(profile.tremolo_depth)),

            clip_pregain: Arc::new(AtomicF32::new(profile.clip_pregain)),
            clip_threshold: Arc::new(AtomicF32::new(profile.clip_threshold)),

            bitcrusher,

            dropout_gen: DropoutGenerator::new(
                profile.dropout_probability,
                profile.dropout_duration_ms,
                sample_rate,
            ),

            sample_rate,

            bypass: false,
        }
    }

    pub fn update_profile(&mut self, profile: &SignalProfile) {
        // Update highpass and lowpass filters separately
        let hp_coeffs = Coefficients::<f32>::from_params(
            biquad::Type::HighPass,
            self.sample_rate.hz(),
            profile.bandpass_low.hz(),
            biquad::Q_BUTTERWORTH_F32,
        )
        .unwrap();

        let lp_coeffs = Coefficients::<f32>::from_params(
            biquad::Type::LowPass,
            self.sample_rate.hz(),
            profile.bandpass_high.hz(),
            biquad::Q_BUTTERWORTH_F32,
        )
        .unwrap();

        // Update coefficients without resetting filter state to avoid transients
        self.highpass_l.update_coefficients(hp_coeffs);
        self.highpass_r.update_coefficients(hp_coeffs);
        self.lowpass_l.update_coefficients(lp_coeffs);
        self.lowpass_r.update_coefficients(lp_coeffs);

        self.white_noise_level
            .store(profile.white_noise_level, Ordering::Relaxed);
        self.pink_noise_level
            .store(profile.pink_noise_level, Ordering::Relaxed);
        self.tremolo_depth
            .store(profile.tremolo_depth, Ordering::Relaxed);
        self.tremolo_lfo
            .set_frequency(self.sample_rate, profile.tremolo_rate);
        self.clip_pregain
            .store(profile.clip_pregain, Ordering::Relaxed);
        self.clip_threshold
            .store(profile.clip_threshold, Ordering::Relaxed);

        if let Some(bits) = profile.bitcrush_bits {
            if let Some(ref mut bc) = self.bitcrusher {
                bc.set_depth(bits);
            } else {
                self.bitcrusher = Some(Bitcrusher::new(bits));
            }
        } else {
            self.bitcrusher = None;
        }

        self.bypass = false;
    }

    pub fn set_bypass(&mut self, bypass: bool) {
        self.bypass = bypass;
    }

    pub fn process_frame(&mut self, input: [f32; 2]) -> [f32; 2] {
        // Bypass all processing if bypass mode is enabled
        if self.bypass {
            return input;
        }

        let mut frame = input;

        // 1. Noise mixing - Add static/interference
        // Mix in generated noise (white and pink) to simulate poor signal
        let (white_noise, pink_noise, _brown_noise) = self.noise_gen.next_frame();
        let white_level = self.white_noise_level.load(Ordering::Relaxed);
        let pink_level = self.pink_noise_level.load(Ordering::Relaxed);
        frame[0] += white_noise[0] * white_level + pink_noise[0] * pink_level;
        frame[1] += white_noise[1] * white_level + pink_noise[1] * pink_level;

        // 2. Highpass + Lowpass filters - Simulate radio frequency response
        // Cuts frequencies outside the desired range
        // e.g., 800Hz highpass + 4500Hz lowpass = "telephone" bandlimiting
        frame[0] = self.highpass_l.run(frame[0]);
        frame[1] = self.highpass_r.run(frame[1]);
        frame[0] = self.lowpass_l.run(frame[0]);
        frame[1] = self.lowpass_r.run(frame[1]);

        // 3. Tremolo - Simulate signal strength fluctuation
        // LFO modulates amplitude to create wavering effect
        let lfo = self.tremolo_lfo.next();
        let depth = self.tremolo_depth.load(Ordering::Relaxed);
        let tremolo_gain = 1.0 - depth + (lfo * 0.5 + 0.5) * depth;
        frame[0] *= tremolo_gain;
        frame[1] *= tremolo_gain;

        // 4. Soft clipping - Simulate overdriven/distorted signal
        // Apply pregain, then clip to threshold to create distortion
        let pregain = self.clip_pregain.load(Ordering::Relaxed);
        let threshold = self.clip_threshold.load(Ordering::Relaxed);
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

fn soft_clip(sample: f32, threshold: f32) -> f32 {
    if sample.abs() < threshold {
        sample
    } else {
        threshold * sample.signum() * (1.0 - (-sample.abs() / threshold).exp())
    }
}
