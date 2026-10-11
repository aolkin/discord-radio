use atomic_float::AtomicF32;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use crate::audio::helpers::perceptual_level_to_amplitude;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TrackHandle(u64);

pub type TrackEndCallback = Arc<dyn Fn(TrackHandle) + Send + Sync>;

pub trait AudioSource: Send + Sync {
    fn next_frame(&mut self) -> Option<[f32; 2]>;
    fn seek(&mut self, position: Duration) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
    #[allow(dead_code)]
    fn duration(&self) -> Option<Duration>;
    fn reset(&mut self);
}

pub struct MixerTrack {
    pub handle: TrackHandle,
    pub source: Box<dyn AudioSource>,
    pub volume: Arc<AtomicF32>,
    pub loops: bool,
    pub active: bool,
    pub end_callback: Option<TrackEndCallback>,
}

impl MixerTrack {
    pub fn new(
        handle: TrackHandle,
        source: Box<dyn AudioSource>,
        volume: f32,
        loops: bool,
        end_callback: Option<TrackEndCallback>,
    ) -> Self {
        // Convert user volume to amplitude using logarithmic scaling
        let amplitude = perceptual_level_to_amplitude(volume);
        Self {
            handle,
            source,
            volume: Arc::new(AtomicF32::new(amplitude)),
            loops,
            active: true,
            end_callback,
        }
    }

    pub fn next_frame(&mut self) -> Option<[f32; 2]> {
        if !self.active {
            return Some([0.0, 0.0]);
        }

        // Limit loop attempts to prevent infinite loops if reset keeps failing
        const MAX_RESET_ATTEMPTS: u32 = 3;
        let mut reset_attempts = 0;

        loop {
            match self.source.next_frame() {
                Some(frame) => {
                    let vol = self.volume.load(Ordering::Relaxed);
                    return Some([frame[0] * vol, frame[1] * vol]);
                }
                None => {
                    if self.loops && reset_attempts < MAX_RESET_ATTEMPTS {
                        self.source.reset();
                        reset_attempts += 1;
                        // Continue loop to try getting a frame from the reset source
                    } else {
                        if reset_attempts >= MAX_RESET_ATTEMPTS {
                            tracing::error!(
                                "Track failed to reset after {} attempts, stopping playback",
                                MAX_RESET_ATTEMPTS
                            );
                        }
                        self.active = false;
                        // Trigger end callback if track ended
                        if let Some(ref callback) = self.end_callback {
                            callback(self.handle);
                        }
                        return Some([0.0, 0.0]);
                    }
                }
            }
        }
    }
}

pub struct CustomMixer {
    tracks: HashMap<TrackHandle, MixerTrack>,
    next_id: u64,
    #[allow(dead_code)]
    sample_rate: u32,
    #[allow(dead_code)]
    channels: u16,
}

/// Every method that takes a [`TrackHandle`] treats a handle whose track was
/// removed or has ended as a no-op and returns `false`.
impl CustomMixer {
    pub fn new(sample_rate: u32) -> Self {
        Self {
            tracks: HashMap::new(),
            next_id: 0,
            sample_rate,
            channels: 2,
        }
    }

    pub fn add_track(
        &mut self,
        source: Box<dyn AudioSource>,
        volume: f32,
        loops: bool,
        on_end: Option<TrackEndCallback>,
    ) -> TrackHandle {
        let handle = TrackHandle(self.next_id);
        self.next_id += 1;
        self.tracks.insert(
            handle,
            MixerTrack::new(handle, source, volume, loops, on_end),
        );
        handle
    }

    pub fn remove_track(&mut self, handle: TrackHandle) -> bool {
        self.tracks.remove(&handle).is_some()
    }

    /// `volume` is user-facing (0.0-2.0) and is converted to amplitude using
    /// perceptual (logarithmic) scaling. 0.0 = silence, 1.0 = unity gain, 2.0 = +18dB boost.
    pub fn set_volume(&mut self, handle: TrackHandle, volume: f32) -> bool {
        let Some(track) = self.tracks.get(&handle) else {
            return false;
        };
        track
            .volume
            .store(perceptual_level_to_amplitude(volume), Ordering::Relaxed);
        true
    }

    pub fn set_loops(&mut self, handle: TrackHandle, loops: bool) -> bool {
        let Some(track) = self.tracks.get_mut(&handle) else {
            return false;
        };
        track.loops = loops;
        true
    }

    pub fn mix_next_frame(&mut self) -> [f32; 2] {
        let mut mixed = [0.0, 0.0];

        for track in self.tracks.values_mut() {
            if let Some(frame) = track.next_frame() {
                mixed[0] += frame[0];
                mixed[1] += frame[1];
            }
        }

        self.tracks.retain(|_, track| track.active || track.loops);

        mixed[0] = mixed[0].clamp(-1.0, 1.0);
        mixed[1] = mixed[1].clamp(-1.0, 1.0);

        mixed
    }
}
