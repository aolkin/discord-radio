use crate::audio::custom_mixer::CustomMixer;
use crate::audio::dsp::chain::RadioEffectChain;
use crate::audio::profiles::SignalProfile;

const SAMPLE_RATE: u32 = 48000;
const FRAME_SIZE: usize = 960; // 20ms at 48kHz
const FRAME_DURATION_MS: u64 = 20;

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

    fn process_transitions(&mut self) {
        if let Some(ref mut transition) = self.transition {
            if let Some(interpolated) = transition.advance(FRAME_DURATION_MS as f32) {
                self.dsp_chain.update_profile(&interpolated);
            }

            if transition.is_complete() {
                self.current_profile = transition.to.clone();
                self.transition = None;
            }
        }
    }

    pub fn fill_buffer(&mut self, buffer: &mut [[f32; 2]]) {
        self.process_transitions();

        for frame in buffer.iter_mut() {
            let mixed = self.mixer.mix_next_frame();
            *frame = self.dsp_chain.process_frame(mixed);
        }
    }

    pub fn process_next_chunk(&mut self) -> Vec<[f32; 2]> {
        let mut buffer = vec![[0.0, 0.0]; FRAME_SIZE];
        self.fill_buffer(&mut buffer);
        buffer
    }
}
