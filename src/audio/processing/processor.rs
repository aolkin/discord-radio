use crate::audio::codec::opus_encoder::OpusEncoder;
use crate::audio::dsp::chain::RadioEffectChain;
use crate::audio::presets::SignalParameters;
use crate::audio::processing::mixer::{AudioSource, Mixer, TrackCommand, TrackHandle};
use crate::logging::LogThrottle;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::broadcast::Receiver;
use tokio::sync::{RwLock, broadcast};

const SAMPLE_RATE: u32 = 48000;
const FRAME_SIZE: usize = 960; // 20ms at 48kHz

const WARN_INTERVAL: Duration = Duration::from_secs(5);
const CHANNEL_CAPACITY: usize = 16; // Buffer up to 16 chunks (320ms at 20ms per chunk)

pub struct SignalParametersTransition {
    pub from: SignalParameters,
    pub to: SignalParameters,
    pub progress: f32,
    pub duration_ms: f32,
}

impl SignalParametersTransition {
    pub fn new(from: SignalParameters, to: SignalParameters, duration_ms: f32) -> Self {
        Self {
            from,
            to,
            progress: 0.0,
            duration_ms,
        }
    }

    pub fn advance(&mut self, delta_ms: f32) -> Option<SignalParameters> {
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

pub type ProcessorRef = Arc<RwLock<AudioProcessor>>;

pub struct AudioProcessor {
    mixer: Mixer,
    dsp_chain: RadioEffectChain,
    transition: Option<SignalParametersTransition>,
    current_preset: SignalParameters,
    samples_processed: u64, // Total stereo frames processed (not individual samples)
    opus_encoder: OpusEncoder,
    opus_stream: broadcast::Sender<Vec<u8>>,
    encode_warnings: LogThrottle,
    slow_warnings: LogThrottle,
}

impl AudioProcessor {
    pub fn new(initial_preset: SignalParameters) -> Self {
        let mixer = Mixer::new(SAMPLE_RATE);
        let dsp_chain = RadioEffectChain::new(SAMPLE_RATE, &initial_preset);
        let opus_encoder = OpusEncoder::default();

        Self {
            mixer,
            dsp_chain,
            transition: None,
            current_preset: initial_preset,
            samples_processed: 0,
            opus_encoder,
            opus_stream: broadcast::channel(CHANNEL_CAPACITY).0,
            encode_warnings: LogThrottle::new(WARN_INTERVAL),
            slow_warnings: LogThrottle::new(WARN_INTERVAL),
        }
    }

    pub fn add_track(
        &mut self,
        source: Box<dyn AudioSource>,
        volume: f32,
        commands: Vec<TrackCommand>,
    ) -> TrackHandle {
        self.mixer.add_track(source, volume, commands)
    }

    pub fn stream_opus(&self) -> Receiver<Vec<u8>> {
        self.opus_stream.subscribe()
    }

    pub fn listener_count(&self) -> usize {
        self.opus_stream.receiver_count()
    }

    pub fn start_preset_transition(&mut self, to_preset: SignalParameters, duration_ms: f32) {
        // If already transitioning, start from the current interpolated position
        // to avoid jarring jumps back to the old preset
        let from_preset = if let Some(ref transition) = self.transition {
            // Get current interpolated preset at this point in the transition
            transition
                .from
                .interpolate(&transition.to, transition.progress)
        } else {
            self.current_preset.clone()
        };

        self.transition = Some(SignalParametersTransition::new(
            from_preset,
            to_preset,
            duration_ms,
        ));
    }

    pub fn set_preset_immediate(&mut self, preset: SignalParameters) {
        self.current_preset = preset.clone();
        self.dsp_chain.update_preset(&preset);
        self.transition = None;
    }

    pub fn current_preset_name(&self) -> &str {
        &self.current_preset.name
    }

    pub fn target_preset(&self) -> &SignalParameters {
        self.transition
            .as_ref()
            .map_or(&self.current_preset, |transition| &transition.to)
    }

    pub fn is_transitioning(&self) -> bool {
        self.transition.is_some()
    }

    fn process_transitions(&mut self, frames_to_render: usize) {
        if let Some(ref mut transition) = self.transition {
            // Calculate time delta based on actual frames being rendered
            // frames_to_render is stereo frames (each frame = L+R sample)
            let delta_ms = (frames_to_render as f32 / SAMPLE_RATE as f32) * 1000.0;

            if let Some(interpolated) = transition.advance(delta_ms) {
                self.dsp_chain.update_preset(&interpolated);
            }

            if transition.is_complete() {
                self.current_preset = transition.to.clone();
                self.transition = None;
            }
        }
    }

    pub fn process_next_chunk(&mut self) -> Vec<[f32; 2]> {
        let start = Instant::now();

        let mut buffer = vec![[0.0, 0.0]; FRAME_SIZE];

        // Advance transitions based on how many frames we're about to render
        self.process_transitions(FRAME_SIZE);
        self.mixer.drain_commands();

        for frame in buffer.iter_mut() {
            let mixed = self.mixer.mix_next_frame();
            *frame = self.dsp_chain.process_frame(mixed);
        }

        // Track total frames processed (stereo frames, not individual samples)
        self.samples_processed += FRAME_SIZE as u64;

        // Encode to Opus and broadcast to web clients
        if self.opus_stream.receiver_count() > 0
            && let Err(e) = self
                .opus_encoder
                .encode(&buffer)
                .map(|packet| self.opus_stream.send(packet))
            && let Some(suppressed) = self.encode_warnings.allow()
        {
            tracing::error!(suppressed, "Opus encoding error: {e:#}");
        }

        // Check if generation is keeping up with real-time
        let elapsed = start.elapsed();
        let audio_duration_ms = (FRAME_SIZE as f64 / SAMPLE_RATE as f64) * 1000.0;
        let generation_time_ms = elapsed.as_secs_f64() * 1000.0;

        // Warn if we're taking longer than the audio duration (not keeping up with real-time)
        if generation_time_ms > audio_duration_ms
            && let Some(suppressed) = self.slow_warnings.allow()
        {
            tracing::warn!(
                suppressed,
                "Audio generation too slow: took {:.2}ms to generate {:.2}ms of audio ({}% real-time)",
                generation_time_ms,
                audio_duration_ms,
                (audio_duration_ms / generation_time_ms * 100.0) as u32
            );
        }

        buffer
    }
}
