use std::collections::{BTreeMap, HashMap};
use std::time::Duration;
use tokio::sync::oneshot;

use crate::audio::helpers::perceptual_level_to_amplitude;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TrackHandle(u64);

pub trait AudioSource: Send + Sync {
    fn next_frame(&mut self) -> Option<[f32; 2]>;
    fn seek(&mut self, position: Duration) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
    #[allow(dead_code)]
    fn duration(&self) -> Option<Duration>;
    fn reset(&mut self);
}

struct Fade {
    target: f32,
    frames: u64,
    stop_at_end: bool,
}

/// Moves a track's user volume linearly to `target`, one `step` per frame.
struct VolumeRamp {
    target: f32,
    step: f32,
    frames_left: u64,
    stop_at_end: bool,
}

pub struct MixerTrack {
    pub source: Box<dyn AudioSource>,
    /// User-facing volume (0.0-2.0).
    volume: f32,
    amplitude: f32,
    played_frames: u64,
    /// Keyed by the played frame at which the fade starts.
    pending_fades: BTreeMap<u64, Fade>,
    ramp: Option<VolumeRamp>,
    pub loops: bool,
    pub active: bool,
    end_tx: Option<oneshot::Sender<()>>,
}

impl MixerTrack {
    fn new(
        source: Box<dyn AudioSource>,
        volume: f32,
        loops: bool,
        end_tx: oneshot::Sender<()>,
    ) -> Self {
        Self {
            source,
            volume,
            amplitude: perceptual_level_to_amplitude(volume),
            played_frames: 0,
            pending_fades: BTreeMap::new(),
            ramp: None,
            loops,
            active: true,
            end_tx: Some(end_tx),
        }
    }

    fn set_volume(&mut self, volume: f32) {
        self.volume = volume;
        self.amplitude = perceptual_level_to_amplitude(volume);
    }

    fn start_due_fades(&mut self) {
        while let Some(entry) = self.pending_fades.first_entry()
            && *entry.key() <= self.played_frames
        {
            let fade = entry.remove();
            self.ramp = Some(VolumeRamp {
                target: fade.target,
                step: (fade.target - self.volume) / fade.frames.max(1) as f32,
                frames_left: fade.frames,
                stop_at_end: fade.stop_at_end,
            });
        }
    }

    /// Returns true when a ramp with `stop_at_end` has just finished.
    fn advance_ramp(&mut self) -> bool {
        let Some(ramp) = &mut self.ramp else {
            return false;
        };
        ramp.frames_left = ramp.frames_left.saturating_sub(1);
        let (target, step, frames_left, stop_at_end) =
            (ramp.target, ramp.step, ramp.frames_left, ramp.stop_at_end);
        if frames_left == 0 {
            self.ramp = None;
            self.set_volume(target);
            stop_at_end
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
                    self.played_frames += 1;
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
                        if let Some(end_tx) = self.end_tx.take() {
                            let _ = end_tx.send(());
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

    /// The receiver gets `Ok` when the track plays to its end, and `Err` when
    /// the track is removed or the mixer is dropped first.
    pub fn add_track(
        &mut self,
        source: Box<dyn AudioSource>,
        volume: f32,
        loops: bool,
    ) -> (TrackHandle, oneshot::Receiver<()>) {
        let handle = TrackHandle(self.next_id);
        self.next_id += 1;

        let (end_tx, end_rx) = oneshot::channel();
        self.tracks
            .insert(handle, MixerTrack::new(source, volume, loops, end_tx));

        (handle, end_rx)
    }

    /// Ramps the track linearly to silence over `fade`, then removes it.
    /// `at` is how long the track has played (frames the mixer has output for
    /// it, across loops) when the ramp starts; `None`, or a time already
    /// reached, starts it before the next frame. A zero `fade` with no `at`
    /// removes the track at once. Returns false if the track does not exist.
    ///
    /// A starting ramp replaces any volume ramp in progress on the track, and a
    /// ramp scheduled for the same frame as a pending one replaces it.
    pub fn remove_track(
        &mut self,
        handle: TrackHandle,
        fade: Duration,
        at: Option<Duration>,
    ) -> bool {
        if fade.is_zero() && at.is_none() {
            return self.tracks.remove(&handle).is_some();
        }
        self.schedule_fade(handle, 0.0, fade, at, true).is_ok()
    }

    /// Update the volume of a track, starting before the next frame.
    ///
    /// * `volume` - User-facing volume (0.0-2.0). This is converted to amplitude using
    ///   perceptual (logarithmic) scaling. 0.0 = silence, 1.0 = unity gain, 2.0 = +18dB boost.
    /// * `fade` - Time to ramp linearly from the current volume. Zero applies it at once.
    ///
    /// Replaces any volume ramp in progress on the track.
    pub fn update_track_volume(
        &mut self,
        handle: TrackHandle,
        volume: f32,
        fade: Duration,
    ) -> anyhow::Result<()> {
        self.schedule_fade(handle, volume, fade, None, false)
    }

    fn schedule_fade(
        &mut self,
        handle: TrackHandle,
        target: f32,
        fade: Duration,
        at: Option<Duration>,
        stop_at_end: bool,
    ) -> anyhow::Result<()> {
        let frames = self.duration_frames(fade);
        let at_frame = at.map(|at| self.duration_frames(at));
        let track = self.track_mut(handle)?;
        track.pending_fades.insert(
            at_frame.unwrap_or(track.played_frames),
            Fade {
                target,
                frames,
                stop_at_end,
            },
        );
        Ok(())
    }

    fn duration_frames(&self, duration: Duration) -> u64 {
        (duration.as_secs_f64() * f64::from(self.sample_rate)).round() as u64
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
            track.start_due_fades();
            if track.advance_ramp() {
                finished_tracks.push(*handle);
                continue;
            }

            if let Some(frame) = track.next_frame() {
                mixed[0] += frame[0];
                mixed[1] += frame[1];
            }

            if !track.active && !track.loops {
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
