use crate::audio::custom_mixer::CustomMixer;
use crate::audio::dsp::chain::RadioEffectChain;
use crate::audio::profiles::SignalProfile;

const SAMPLE_RATE: u32 = 48000;
const FRAME_SIZE: usize = 960; // 20ms at 48kHz

pub struct ProfileTransition {
    pub from: SignalProfile,
    pub to: SignalProfile,
    pub progress: f32,
    pub duration_ms: f32,
}

impl ProfileTransition {
    pub fn new(from: SignalProfile, to: SignalProfile, duration_ms: f32) -> Self {
        Self {
            from,
            to,
            progress: 0.0,
            duration_ms,
        }
    }

    pub fn advance(&mut self, delta_ms: f32) -> Option<SignalProfile> {
        if self.progress >= 1.0 {
            return None;
        }

        self.progress += delta_ms / self.duration_ms;
        self.progress = self.progress.min(1.0);

        Some(self.from.interpolate(&self.to, self.progress))
    }

    pub fn is_complete(&self) -> bool {
        self.progress >= 1.0
    }
}

pub struct AudioProcessor {
    mixer: CustomMixer,
    dsp_chain: RadioEffectChain,
    transition: Option<ProfileTransition>,
    current_profile: SignalProfile,
    samples_processed: u64, // Total stereo frames processed (not individual samples)
}

impl AudioProcessor {
    pub fn new(initial_profile: SignalProfile) -> Self {
        let mixer = CustomMixer::new(SAMPLE_RATE);
        let dsp_chain = RadioEffectChain::new(SAMPLE_RATE, &initial_profile);

        Self {
            mixer,
            dsp_chain,
            transition: None,
            current_profile: initial_profile,
            samples_processed: 0,
        }
    }

    pub fn mixer_mut(&mut self) -> &mut CustomMixer {
        &mut self.mixer
    }

    pub fn start_profile_transition(&mut self, to_profile: SignalProfile, duration_ms: f32) {
        self.transition = Some(ProfileTransition::new(
            self.current_profile.clone(),
            to_profile,
            duration_ms,
        ));
    }

    pub fn set_profile_immediate(&mut self, profile: SignalProfile) {
        self.current_profile = profile.clone();
        self.dsp_chain.update_profile(&profile);
        self.transition = None;
    }

    pub fn set_bypass(&mut self, bypass: bool) {
        self.dsp_chain.set_bypass(bypass);
        self.transition = None;
    }

    fn process_transitions(&mut self, frames_to_render: usize) {
        if let Some(ref mut transition) = self.transition {
            // Calculate time delta based on actual frames being rendered
            // frames_to_render is stereo frames (each frame = L+R sample)
            let delta_ms = (frames_to_render as f32 / SAMPLE_RATE as f32) * 1000.0;

            if let Some(interpolated) = transition.advance(delta_ms) {
                self.dsp_chain.update_profile(&interpolated);
            }

            if transition.is_complete() {
                self.current_profile = transition.to.clone();
                self.transition = None;
            }
        }
    }

    pub fn process_next_chunk(&mut self) -> Vec<[f32; 2]> {
        let start = std::time::Instant::now();

        let mut buffer = vec![[0.0, 0.0]; FRAME_SIZE];

        // Advance transitions based on how many frames we're about to render
        self.process_transitions(FRAME_SIZE);

        for frame in buffer.iter_mut() {
            let mixed = self.mixer.mix_next_frame();
            *frame = self.dsp_chain.process_frame(mixed);
        }

        // Track total frames processed (stereo frames, not individual samples)
        self.samples_processed += FRAME_SIZE as u64;

        // Check if generation is keeping up with real-time
        let elapsed = start.elapsed();
        let audio_duration_ms = (FRAME_SIZE as f64 / SAMPLE_RATE as f64) * 1000.0;
        let generation_time_ms = elapsed.as_secs_f64() * 1000.0;

        // Warn if we're taking longer than the audio duration (not keeping up with real-time)
        if generation_time_ms > audio_duration_ms {
            tracing::warn!(
                "Audio generation too slow: took {:.2}ms to generate {:.2}ms of audio ({}% real-time)",
                generation_time_ms,
                audio_duration_ms,
                (audio_duration_ms / generation_time_ms * 100.0) as u32
            );
        }

        buffer
    }
}
