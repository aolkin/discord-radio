use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};

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

const COMMAND_QUEUE_SIZE: usize = 10;

/// An edit to a track, sent with [`TrackHandle::send`]. The mixer applies
/// edits at the start of its next chunk, in the order they were sent.
pub enum TrackCommand {
    /// * `volume` - User-facing volume (0.0-2.0). This is converted to amplitude using
    ///   perceptual (logarithmic) scaling. 0.0 = silence, 1.0 = unity gain, 2.0 = +18dB boost.
    /// * `fade` - Time to ramp linearly from the current volume. Zero applies it at once.
    /// * `at` - When the ramp starts (see [`Self::Stop`]).
    ///
    /// Replaces any volume ramp in progress on the track.
    SetVolume {
        volume: f32,
        fade: Duration,
        at: Option<Duration>,
    },
    SetLoops(bool),
    /// Sets the track's stop, replacing any earlier one. Once the track has
    /// played for `at` (frames the mixer has output for it, across loops;
    /// `None` means now), it ramps linearly to silence over `fade`, then
    /// stops, overwriting any in-progress volume ramp at the time.
    Stop {
        fade: Duration,
        at: Option<Duration>,
    },
    /// The receiver of `tx` gets `()` once the track has played for `at` (see
    /// [`Self::Stop`]), or an error if the track ends first.
    AddTrigger {
        at: Duration,
        tx: oneshot::Sender<()>,
    },
    /// The receiver gets the reason the track ends, or an error if the
    /// track is lost without one.
    AddStopTrigger(oneshot::Sender<StopReason>),
}

/// A track added to the mixer.
#[derive(Clone, Debug)]
pub struct TrackHandle {
    pub id: TrackId,
    sample_rate: u32,
    played_frames: Arc<AtomicU64>,
    commands: mpsc::Sender<TrackCommand>,
}

impl TrackHandle {
    /// Returns `Ok(false)` if the track has ended, and an error if its
    /// command queue is full.
    pub fn send(&self, command: TrackCommand) -> anyhow::Result<bool> {
        match self.commands.try_send(command) {
            Ok(()) => Ok(true),
            Err(mpsc::error::TrySendError::Closed(_)) => Ok(false),
            Err(mpsc::error::TrySendError::Full(_)) => {
                anyhow::bail!("Track command queue is full")
            }
        }
    }

    /// Total frames played, including loops.
    pub fn elapsed(&self) -> Duration {
        Duration::from_secs_f64(
            self.played_frames.load(Ordering::Relaxed) as f64 / f64::from(self.sample_rate),
        )
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
    start: u64,
    end: u64,
}

/// Moves a track's user volume linearly from `from` to `target` over
/// `frames`, starting at played frame `start`.
struct VolumeRamp {
    from: f32,
    to: f32,
    start: u64,
    end: u64,
}

impl Stop {
    fn matches_ramp(&self, ramp: &Option<VolumeRamp>) -> bool {
        ramp.as_ref()
            .is_some_and(|ramp| ramp.to == 0.0 && ramp.end == self.end)
    }
}

pub struct MixerTrack {
    pub source: Box<dyn AudioSource>,
    /// User-facing volume (0.0-2.0).
    volume: f32,
    amplitude: f32,
    played_frames: Arc<AtomicU64>,
    /// Keyed by the played frame at which the keyframe applies, then by the
    /// order the keyframes were added.
    keyframes: BTreeMap<(u64, u64), Keyframe>,
    next_keyframe_id: u64,
    stop: Option<Stop>,
    ramp: Option<VolumeRamp>,
    pub loops: bool,
    pub active: bool,
    commands: mpsc::Receiver<TrackCommand>,
    stop_triggers: Vec<oneshot::Sender<StopReason>>,
}

impl MixerTrack {
    fn new(
        source: Box<dyn AudioSource>,
        volume: f32,
        played_frames: Arc<AtomicU64>,
        commands: mpsc::Receiver<TrackCommand>,
    ) -> Self {
        Self {
            source,
            volume,
            amplitude: perceptual_level_to_amplitude(volume),
            played_frames,
            keyframes: BTreeMap::new(),
            next_keyframe_id: 0,
            stop: None,
            ramp: None,
            loops: false,
            active: true,
            commands,
            stop_triggers: Vec::new(),
        }
    }

    fn apply_commands(&mut self, sample_rate: u32) {
        while let Ok(command) = self.commands.try_recv() {
            self.apply(command, sample_rate);
        }
    }

    fn apply(&mut self, command: TrackCommand, sample_rate: u32) {
        let frames =
            |duration: Duration| (duration.as_secs_f64() * f64::from(sample_rate)).round() as u64;
        match command {
            TrackCommand::SetVolume { volume, fade, at } => self.add_keyframe(
                self.start_frame(at.map(frames)),
                Keyframe::StartFade {
                    target: volume,
                    frames: frames(fade),
                },
            ),
            TrackCommand::SetLoops(loops) => self.loops = loops,
            TrackCommand::Stop { fade, at } => self.stop(frames(fade), at.map(frames)),
            TrackCommand::AddTrigger { at, tx } => {
                self.add_keyframe(frames(at), Keyframe::Trigger(tx))
            }
            TrackCommand::AddStopTrigger(tx) => self.stop_triggers.push(tx),
        }
    }

    fn played_frames(&self) -> u64 {
        self.played_frames.load(Ordering::Relaxed)
    }

    fn start_frame(&self, at: Option<u64>) -> u64 {
        let played_frames = self.played_frames();
        at.map_or(played_frames, |at| at.max(played_frames))
    }

    fn stop(&mut self, fade: u64, at: Option<u64>) {
        let start = self.start_frame(at);
        let new_stop = Stop {
            start,
            end: start + fade,
        };
        if self
            .stop
            .replace(new_stop)
            .is_some_and(|old| old.matches_ramp(&self.ramp))
        {
            self.ramp = None;
        }
    }

    /// Only the first call has an effect.
    fn end(&mut self, reason: StopReason) {
        for tx in self.stop_triggers.drain(..) {
            let _ = tx.send(reason);
        }
    }

    fn add_keyframe(&mut self, frame: u64, keyframe: Keyframe) {
        self.keyframes
            .insert((frame, self.next_keyframe_id), keyframe);
        self.next_keyframe_id += 1;
    }

    fn active_stop(&self) -> Option<&Stop> {
        self.stop
            .as_ref()
            .filter(|stop| self.played_frames() >= stop.start)
    }

    /// Skips `StartFade` keyframes while the stop's fade runs.
    fn apply_due_keyframes(&mut self) {
        let played_frames = self.played_frames();
        while let Some(entry) = self.keyframes.first_entry()
            && entry.key().0 <= played_frames
        {
            match entry.remove() {
                Keyframe::StartFade { target, frames } => {
                    if self.active_stop().is_none() {
                        self.ramp = Some(VolumeRamp {
                            from: self.volume,
                            to: target,
                            start: played_frames,
                            end: played_frames + frames,
                        });
                    }
                }
                Keyframe::Trigger(tx) => {
                    let _ = tx.send(());
                }
            }
        }
        if let Some(stop) = self.active_stop()
            && !stop.matches_ramp(&self.ramp)
        {
            self.ramp = Some(VolumeRamp {
                from: self.volume,
                to: 0.0,
                start: stop.start,
                end: stop.end,
            });
        }
    }

    fn update_amplitude(&mut self) {
        let played_frames = self.played_frames();
        if let Some(ramp) = &self.ramp {
            if played_frames >= ramp.end {
                self.volume = ramp.to;
                self.ramp = None;
            } else {
                let elapsed = played_frames.saturating_sub(ramp.start);
                let progress = elapsed as f32 / (ramp.end - ramp.start) as f32;
                self.volume = ramp.from + (ramp.to - ramp.from) * progress;
            }
            self.amplitude = perceptual_level_to_amplitude(self.volume);
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
                    self.played_frames.fetch_add(1, Ordering::Relaxed);
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
}

impl Mixer {
    pub fn new(sample_rate: u32) -> Self {
        Self {
            tracks: HashMap::new(),
            next_id: 1,
            sample_rate,
        }
    }

    /// Applies `commands` in order before the mixer plays the track.
    pub fn add_track(
        &mut self,
        source: Box<dyn AudioSource>,
        volume: f32,
        commands: Vec<TrackCommand>,
    ) -> TrackHandle {
        let handle = TrackId(self.next_id);
        self.next_id += 1;

        let (commands_tx, commands_rx) = mpsc::channel(COMMAND_QUEUE_SIZE);
        let played_frames = Arc::new(AtomicU64::new(0));
        let mut track = MixerTrack::new(source, volume, played_frames.clone(), commands_rx);
        for command in commands {
            track.apply(command, self.sample_rate);
        }
        self.tracks.insert(handle, track);
        TrackHandle {
            id: handle,
            sample_rate: self.sample_rate,
            played_frames,
            commands: commands_tx,
        }
    }

    /// Removes the track at once. Returns false if the track does not exist.
    #[allow(dead_code)]
    pub fn remove_track(&mut self, handle: TrackId) -> bool {
        self.tracks
            .remove(&handle)
            .map(|mut track| track.end(StopReason::Removed))
            .is_some()
    }

    /// Applies the edits sent through each track's [`TrackHandle`].
    pub fn drain_commands(&mut self) {
        for track in self.tracks.values_mut() {
            track.apply_commands(self.sample_rate);
        }
    }

    pub fn mix_next_frame(&mut self) -> [f32; 2] {
        let mut mixed = [0.0, 0.0];
        let mut finished_tracks = Vec::new();

        for (handle, track) in self.tracks.iter_mut() {
            track.apply_due_keyframes();
            if track
                .active_stop()
                .is_some_and(|stop| track.played_frames() >= stop.end)
            {
                track.end(StopReason::Stopped);
                finished_tracks.push(*handle);
                continue;
            }
            track.update_amplitude();

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

#[cfg(test)]
mod tests;
