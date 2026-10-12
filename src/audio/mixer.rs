use std::collections::{BTreeMap, HashMap};
use std::time::Duration;
use tokio::sync::{oneshot, watch};

use crate::audio::helpers::perceptual_level_to_amplitude;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TrackId(u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopReason {
    /// A source played to its end.
    Completed,
    /// Removed directly by `remove_track`, bypassing any stop.
    Removed,
    /// Stopped at a requested time.
    Stopped,
    /// A looping source failed to restart.
    FailedReset,
    /// Dropped before any other reason was set.
    Dropped,
    /// The end signal was lost.
    Lost,
}

/// A track added to the mixer. It has ended once it plays to its end, is
/// removed or stopped, or the mixer is dropped.
#[derive(Clone, Debug)]
pub struct TrackHandle {
    pub id: TrackId,
    /// Set once, when the track ends.
    end_rx: watch::Receiver<Option<StopReason>>,
}

impl TrackHandle {
    pub fn has_ended(&self) -> bool {
        self.stop_reason().is_some()
    }

    pub fn stop_reason(&self) -> Option<StopReason> {
        *self.end_rx.borrow()
    }

    pub async fn ended(&self) -> StopReason {
        match self.end_rx.clone().wait_for(Option::is_some).await {
            Ok(reason) => reason.unwrap_or(StopReason::Lost),
            Err(e) => {
                tracing::error!("Track {:?} lost its end signal: {e}", self.id);
                StopReason::Lost
            }
        }
    }
}

pub trait AudioSource: Send + Sync {
    fn next_frame(&mut self) -> Option<[f32; 2]>;
    fn seek(&mut self, position: Duration) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
    #[allow(dead_code)]
    fn duration(&self) -> Option<Duration>;
    fn reset(&mut self);
}

enum Keyframe {
    StartFade { target: f32, frames: u64 },
    Trigger(oneshot::Sender<()>),
}

struct Stop {
    at: u64,
    fade: u64,
    fading: bool,
}

/// Moves a track's user volume linearly to `target`, one `step` per frame.
struct VolumeRamp {
    target: f32,
    step: f32,
    frames_left: u64,
}

pub struct MixerTrack {
    pub source: Box<dyn AudioSource>,
    /// User-facing volume (0.0-2.0).
    volume: f32,
    amplitude: f32,
    played_frames: u64,
    /// Keyed by the played frame at which the keyframe applies, then by the
    /// order the keyframes were added.
    keyframes: BTreeMap<(u64, u64), Keyframe>,
    next_keyframe: u64,
    stop: Option<Stop>,
    ramp: Option<VolumeRamp>,
    pub loops: bool,
    pub active: bool,
    end_tx: watch::Sender<Option<StopReason>>,
}

impl MixerTrack {
    fn new(
        source: Box<dyn AudioSource>,
        volume: f32,
        loops: bool,
        end_tx: watch::Sender<Option<StopReason>>,
    ) -> Self {
        Self {
            source,
            volume,
            amplitude: perceptual_level_to_amplitude(volume),
            played_frames: 0,
            keyframes: BTreeMap::new(),
            next_keyframe: 0,
            stop: None,
            ramp: None,
            loops,
            active: true,
            end_tx,
        }
    }

    fn set_volume(&mut self, volume: f32) {
        self.volume = volume;
        self.amplitude = perceptual_level_to_amplitude(volume);
    }

    /// Only the first call has an effect.
    fn end(&mut self, reason: StopReason) {
        self.end_tx.send_modify(|end| {
            end.get_or_insert(reason);
        });
    }

    fn add_keyframe(&mut self, frame: u64, keyframe: Keyframe) {
        self.keyframes.insert((frame, self.next_keyframe), keyframe);
        self.next_keyframe += 1;
    }

    fn start_ramp(&mut self, target: f32, frames: u64) {
        self.ramp = Some(VolumeRamp {
            target,
            step: (target - self.volume) / frames.max(1) as f32,
            frames_left: frames,
        });
    }

    /// Skips `StartFade` keyframes while the stop's fade runs.
    fn apply_due_keyframes(&mut self) {
        while let Some(entry) = self.keyframes.first_entry()
            && entry.key().0 <= self.played_frames
        {
            match entry.remove() {
                Keyframe::StartFade { target, frames } => {
                    if !self.stop.as_ref().is_some_and(|stop| stop.fading) {
                        self.start_ramp(target, frames);
                    }
                }
                Keyframe::Trigger(tx) => {
                    let _ = tx.send(());
                }
            }
        }
    }

    /// Returns true when the track should stop now.
    fn stop_due(&mut self) -> bool {
        let Some(stop) = &mut self.stop else {
            return false;
        };
        if stop.fading {
            return self.ramp.is_none();
        }
        if self.played_frames < stop.at {
            return false;
        }
        if stop.fade == 0 {
            return true;
        }
        stop.fading = true;
        let fade = stop.fade;
        self.start_ramp(0.0, fade);
        false
    }

    fn advance_ramp(&mut self) {
        let Some(ramp) = &mut self.ramp else {
            return;
        };
        ramp.frames_left = ramp.frames_left.saturating_sub(1);
        let (target, step, frames_left) = (ramp.target, ramp.step, ramp.frames_left);
        if frames_left == 0 {
            self.ramp = None;
            self.set_volume(target);
        } else {
            self.set_volume(target - step * frames_left as f32);
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
                        let reason = if reset_attempts >= MAX_RESET_ATTEMPTS {
                            tracing::error!(
                                "Track failed to reset after {} attempts, stopping playback",
                                MAX_RESET_ATTEMPTS
                            );
                            StopReason::FailedReset
                        } else {
                            StopReason::Completed
                        };
                        self.active = false;
                        self.end(reason);
                        return Some([0.0, 0.0]);
                    }
                }
            }
        }
    }
}

impl Drop for MixerTrack {
    fn drop(&mut self) {
        self.end(StopReason::Dropped);
    }
}

pub struct Mixer {
    tracks: HashMap<TrackId, MixerTrack>,
    next_id: u64,
    sample_rate: u32,
    #[allow(dead_code)]
    channels: u16,
}

/// A [`TrackId`] whose track was removed or has ended is a no-op for
/// `remove_track` and `stop_track`, and the `update_*` methods return an error for it.
impl Mixer {
    pub fn new(sample_rate: u32) -> Self {
        Self {
            tracks: HashMap::new(),
            next_id: 1,
            sample_rate,
            channels: 2,
        }
    }

    pub fn add_track(
        &mut self,
        source: Box<dyn AudioSource>,
        volume: f32,
        loops: bool,
    ) -> TrackHandle {
        let handle = TrackId(self.next_id);
        self.next_id += 1;

        let (end_tx, end_rx) = watch::channel(None);
        self.tracks
            .insert(handle, MixerTrack::new(source, volume, loops, end_tx));

        TrackHandle { id: handle, end_rx }
    }

    /// Removes the track at once. Returns false if the track does not exist.
    pub fn remove_track(&mut self, handle: TrackId) -> bool {
        self.tracks
            .remove(&handle)
            .map(|mut track| track.end(StopReason::Removed))
            .is_some()
    }

    /// Sets the track's stop, replacing any earlier one. Once the track has
    /// played for `at` (frames the mixer has output for it, across loops;
    /// `None` means now), it ramps linearly to silence over `fade`, then
    /// stops. A zero `fade` stops it at `at`, after the keyframes due then.
    /// The ramp replaces any volume ramp in progress, and volume changes
    /// are ignored while it runs. Returns false if the track does not exist.
    pub fn stop_track(&mut self, handle: TrackId, fade: Duration, at: Option<Duration>) -> bool {
        let fade = self.duration_frames(fade);
        let at = at.map(|at| self.duration_frames(at));
        let Ok(track) = self.track_mut(handle) else {
            return false;
        };
        track.stop = Some(Stop {
            at: at.unwrap_or(track.played_frames),
            fade,
            fading: false,
        });
        true
    }

    /// The receiver gets `()` once the track has played for `at` (see
    /// [`Self::stop_track`]), or an error if the track ends first.
    pub fn add_trigger(
        &mut self,
        handle: TrackId,
        at: Duration,
    ) -> anyhow::Result<oneshot::Receiver<()>> {
        let at_frame = self.duration_frames(at);
        let (tx, rx) = oneshot::channel();
        self.track_mut(handle)?
            .add_keyframe(at_frame, Keyframe::Trigger(tx));
        Ok(rx)
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
        handle: TrackId,
        volume: f32,
        fade: Duration,
    ) -> anyhow::Result<()> {
        let frames = self.duration_frames(fade);
        let track = self.track_mut(handle)?;
        let start = track.played_frames;
        track.add_keyframe(
            start,
            Keyframe::StartFade {
                target: volume,
                frames,
            },
        );
        Ok(())
    }

    fn duration_frames(&self, duration: Duration) -> u64 {
        (duration.as_secs_f64() * f64::from(self.sample_rate)).round() as u64
    }

    pub fn update_track_loops(&mut self, handle: TrackId, loops: bool) -> anyhow::Result<()> {
        self.track_mut(handle)?.loops = loops;
        Ok(())
    }

    fn track_mut(&mut self, handle: TrackId) -> anyhow::Result<&mut MixerTrack> {
        self.tracks
            .get_mut(&handle)
            .ok_or_else(|| anyhow::anyhow!("Track {handle:?} not found"))
    }

    pub fn mix_next_frame(&mut self) -> [f32; 2] {
        let mut mixed = [0.0, 0.0];
        let mut finished_tracks = Vec::new();

        for (handle, track) in self.tracks.iter_mut() {
            track.apply_due_keyframes();
            if track.stop_due() {
                track.end(StopReason::Stopped);
                finished_tracks.push(*handle);
                continue;
            }
            track.advance_ramp();

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
