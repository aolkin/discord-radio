use std::collections::HashMap;
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

/// Moves a track's volume linearly from its current value to `target` over
/// `duration`. A zero `duration` sets the volume immediately.
/// `stop_when_complete` removes the track once `target` is reached.
#[derive(Clone, Copy, Debug)]
pub struct Fade {
    pub target: f32,
    pub duration: Duration,
    pub stop_when_complete: bool,
}

struct ScheduledFade {
    at_frame: u64,
    target: f32,
    frames: u64,
    stop_when_complete: bool,
}

/// Moves a track's user volume linearly to `target`, one `step` per frame.
struct VolumeRamp {
    target: f32,
    step: f32,
    frames_left: u64,
    stop_when_complete: bool,
}

pub struct MixerTrack {
    pub source: Box<dyn AudioSource>,
    /// User-facing volume (0.0-2.0).
    volume: f32,
    amplitude: f32,
    played_frames: u64,
    /// Sorted by `at_frame`, latest first.
    pending_fades: Vec<ScheduledFade>,
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
            pending_fades: Vec::new(),
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

    fn schedule_fade(&mut self, fade: ScheduledFade) {
        let index = self
            .pending_fades
            .partition_point(|pending| pending.at_frame > fade.at_frame);
        self.pending_fades.insert(index, fade);
    }

    /// Returns true when a fade with `stop_when_complete` has completed.
    fn start_fade(&mut self, fade: ScheduledFade) -> bool {
        if fade.frames == 0 {
            self.ramp = None;
            self.set_volume(fade.target);
            return fade.stop_when_complete;
        }
        self.ramp = Some(VolumeRamp {
            target: fade.target,
            step: (fade.target - self.volume) / fade.frames as f32,
            frames_left: fade.frames,
            stop_when_complete: fade.stop_when_complete,
        });
        false
    }

    /// Returns true when a fade with `stop_when_complete` has completed.
    fn start_due_fades(&mut self) -> bool {
        let mut complete = false;
        while let Some(fade) = self
            .pending_fades
            .pop_if(|fade| fade.at_frame <= self.played_frames)
        {
            complete |= self.start_fade(fade);
        }
        complete
    }

    /// Returns true when a ramp with `stop_when_complete` has just finished.
    fn advance_ramp(&mut self) -> bool {
        let Some(ramp) = &mut self.ramp else {
            return false;
        };
        ramp.frames_left -= 1;
        let (target, step, frames_left, stop_when_complete) = (
            ramp.target,
            ramp.step,
            ramp.frames_left,
            ramp.stop_when_complete,
        );
        if frames_left == 0 {
            self.ramp = None;
            self.set_volume(target);
            stop_when_complete
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
/// `remove_track`, and `add_fade` and the `update_*` methods return an error
/// for it.
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

    /// Removes the track immediately. Returns false if the track does not exist.
    pub fn remove_track(&mut self, handle: TrackHandle) -> bool {
        self.tracks.remove(&handle).is_some()
    }

    /// Schedules `fade` to start once the track has played for `at` (the
    /// frames the mixer has output for it, across loops). An `at` the track
    /// has already reached starts the fade now. A starting fade replaces any
    /// fade in progress on the track.
    ///
    /// Volumes are user-facing (0.0-2.0), converted to amplitude using
    /// perceptual (logarithmic) scaling: 0.0 = silence, 1.0 = unity gain,
    /// 2.0 = +18dB boost.
    pub fn add_fade(
        &mut self,
        handle: TrackHandle,
        at: Duration,
        fade: Fade,
    ) -> anyhow::Result<()> {
        let scheduled = ScheduledFade {
            at_frame: self.duration_frames(at),
            target: fade.target,
            frames: self.duration_frames(fade.duration),
            stop_when_complete: fade.stop_when_complete,
        };
        let track = self.track_mut(handle)?;
        if scheduled.at_frame > track.played_frames {
            track.schedule_fade(scheduled);
        } else if track.start_fade(scheduled) {
            self.tracks.remove(&handle);
        }
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
            if track.start_due_fades() {
                finished_tracks.push(*handle);
                continue;
            }

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
