use std::collections::HashMap;
use std::sync::Arc;
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

/// Moves a track's user volume linearly to `target`, one `step` per frame.
struct VolumeRamp {
    target: f32,
    step: f32,
    frames_left: u32,
    remove_at_end: bool,
}

pub struct MixerTrack {
    pub handle: TrackHandle,
    pub source: Box<dyn AudioSource>,
    /// User-facing volume (0.0-2.0).
    volume: f32,
    amplitude: f32,
    ramp: Option<VolumeRamp>,
    pub loops: bool,
    pub active: bool,
    pub end_callback: Option<TrackEndCallback>,
}

impl MixerTrack {
    fn new(handle: TrackHandle, source: Box<dyn AudioSource>, volume: f32, loops: bool) -> Self {
        Self {
            handle,
            source,
            volume,
            amplitude: perceptual_level_to_amplitude(volume),
            ramp: None,
            loops,
            active: true,
            end_callback: None,
        }
    }

    pub fn with_end_callback(mut self, callback: TrackEndCallback) -> Self {
        self.end_callback = Some(callback);
        self
    }

    fn set_volume(&mut self, volume: f32) {
        self.volume = volume;
        self.amplitude = perceptual_level_to_amplitude(volume);
    }

    fn start_ramp(&mut self, target: f32, frames: u32, remove_at_end: bool) {
        let frames = frames.max(1);
        self.ramp = Some(VolumeRamp {
            target,
            step: (target - self.volume) / frames as f32,
            frames_left: frames,
            remove_at_end,
        });
    }

    /// Returns true when a ramp with `remove_at_end` has just finished.
    fn advance_ramp(&mut self) -> bool {
        let Some(ramp) = &mut self.ramp else {
            return false;
        };
        ramp.frames_left -= 1;
        let (target, step, frames_left, remove_at_end) =
            (ramp.target, ramp.step, ramp.frames_left, ramp.remove_at_end);
        if frames_left == 0 {
            self.ramp = None;
            self.set_volume(target);
            remove_at_end
        } else {
            self.set_volume(target - step * frames_left as f32);
            false
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
                    return Some([frame[0] * self.amplitude, frame[1] * self.amplitude]);
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
    sample_rate: u32,
    #[allow(dead_code)]
    channels: u16,
}

/// A [`TrackHandle`] whose track was removed or has ended is a no-op for
/// `remove_track`, and the `update_*` methods return an error for it.
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
        callback: Option<TrackEndCallback>,
    ) -> TrackHandle {
        let handle = TrackHandle(self.next_id);
        self.next_id += 1;

        let mut track = MixerTrack::new(handle, source, volume, loops);
        if let Some(cb) = callback {
            track = track.with_end_callback(cb);
        }
        self.tracks.insert(handle, track);

        handle
    }

    pub fn remove_track(&mut self, handle: TrackHandle) -> bool {
        self.tracks.remove(&handle).is_some()
    }

    /// Update the volume of a track.
    ///
    /// # Arguments
    ///
    /// * `handle` - Handle of the track to update
    /// * `volume` - User-facing volume (0.0-2.0). This is converted to amplitude using
    ///   perceptual (logarithmic) scaling. 0.0 = silence, 1.0 = unity gain, 2.0 = +18dB boost.
    ///
    /// # Returns
    ///
    /// Ok(()) if successful, or an error if the track is not found.
    ///
    /// Cancels any volume ramp in progress on the track.
    pub fn update_track_volume(&mut self, handle: TrackHandle, volume: f32) -> anyhow::Result<()> {
        let track = self.track_mut(handle)?;
        track.ramp = None;
        track.set_volume(volume);
        Ok(())
    }

    /// Ramps the track's user volume linearly from its current value to `volume`
    /// over `duration`, replacing any ramp in progress.
    pub fn ramp_track_volume(
        &mut self,
        handle: TrackHandle,
        volume: f32,
        duration: Duration,
    ) -> anyhow::Result<()> {
        let frames = self.duration_frames(duration);
        self.track_mut(handle)?.start_ramp(volume, frames, false);
        Ok(())
    }

    /// Ramps the track to silence over `duration`, then removes it without
    /// invoking its end callback. Returns false if the track does not exist.
    pub fn fade_out_and_remove_track(&mut self, handle: TrackHandle, duration: Duration) -> bool {
        let frames = self.duration_frames(duration);
        match self.tracks.get_mut(&handle) {
            Some(track) => {
                track.start_ramp(0.0, frames, true);
                true
            }
            None => false,
        }
    }

    fn duration_frames(&self, duration: Duration) -> u32 {
        (duration.as_secs_f64() * f64::from(self.sample_rate)).round() as u32
    }

    pub fn update_track_loops(&mut self, handle: TrackHandle, loops: bool) -> anyhow::Result<()> {
        self.track_mut(handle)?.loops = loops;
        Ok(())
    }

    fn track_mut(&mut self, handle: TrackHandle) -> anyhow::Result<&mut MixerTrack> {
        self.tracks
            .get_mut(&handle)
            .ok_or_else(|| anyhow::anyhow!("Track {handle:?} not found"))
    }

    pub fn mix_next_frame(&mut self) -> [f32; 2] {
        let mut mixed = [0.0, 0.0];
        let mut finished_tracks = Vec::new();

        for (handle, track) in self.tracks.iter_mut() {
            if let Some(frame) = track.next_frame() {
                mixed[0] += frame[0];
                mixed[1] += frame[1];
            }

            let faded_out = track.advance_ramp();
            if faded_out || (!track.active && !track.loops) {
                finished_tracks.push(*handle);
            }
        }

        for handle in finished_tracks {
            self.tracks.remove(&handle);
        }

        mixed[0] = mixed[0].clamp(-1.0, 1.0);
        mixed[1] = mixed[1].clamp(-1.0, 1.0);

        mixed
    }
}
