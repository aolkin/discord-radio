use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::sync::{mpsc, oneshot, watch};

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
    AddTrigger {
        at: Duration,
        tx: oneshot::Sender<()>,
    },
}

impl TrackCommand {
    /// The receiver gets `()` once the track has played for `at` (see
    /// [`Self::Stop`]), or an error if the track ends first.
    pub fn add_trigger(at: Duration) -> (Self, oneshot::Receiver<()>) {
        let (tx, rx) = oneshot::channel();
        (Self::AddTrigger { at, tx }, rx)
    }
}

/// A track added to the mixer. It has ended once it plays to its end, is
/// removed or stopped, or the mixer is dropped.
#[derive(Clone, Debug)]
pub struct TrackHandle {
    pub id: TrackId,
    sample_rate: u32,
    played_frames: Arc<AtomicU64>,
    commands: mpsc::Sender<TrackCommand>,
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

    /// Returns `Ok(false)` if the track has ended, and an error if its
    /// command queue is full.
    pub fn send(&self, command: TrackCommand) -> anyhow::Result<bool> {
        match self.commands.try_send(command) {
            Ok(()) => Ok(true),
            Err(mpsc::error::TrySendError::Closed(_)) => Ok(false),
            Err(mpsc::error::TrySendError::Full(_)) => {
                anyhow::bail!("Track {:?} command queue is full", self.id)
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

pub struct MixerTrackArgs {
    pub volume: f32,
    pub loops: bool,
    pub fade_in: Duration,
}

impl Default for MixerTrackArgs {
    fn default() -> Self {
        Self {
            volume: 1.0,
            loops: false,
            fade_in: Duration::ZERO,
        }
    }
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
    end_tx: watch::Sender<Option<StopReason>>,
}

impl MixerTrack {
    fn new(
        source: Box<dyn AudioSource>,
        volume: f32,
        loops: bool,
        played_frames: Arc<AtomicU64>,
        commands: mpsc::Receiver<TrackCommand>,
        end_tx: watch::Sender<Option<StopReason>>,
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
            loops,
            active: true,
            commands,
            end_tx,
        }
    }

    fn apply_commands(&mut self, sample_rate: u32) {
        let frames =
            |duration: Duration| (duration.as_secs_f64() * f64::from(sample_rate)).round() as u64;
        while let Ok(command) = self.commands.try_recv() {
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
            }
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
        self.end_tx.send_modify(|end| {
            end.get_or_insert(reason);
        });
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
    #[allow(dead_code)]
    channels: u16,
}

impl Mixer {
    pub fn new(sample_rate: u32) -> Self {
        Self {
            tracks: HashMap::new(),
            next_id: 1,
            sample_rate,
            channels: 2,
        }
    }

    pub fn add_track(&mut self, source: Box<dyn AudioSource>, args: MixerTrackArgs) -> TrackHandle {
        let handle = TrackId(self.next_id);
        self.next_id += 1;

        let (commands_tx, commands_rx) = mpsc::channel(COMMAND_QUEUE_SIZE);
        let (end_tx, end_rx) = watch::channel(None);
        let played_frames = Arc::new(AtomicU64::new(0));
        let mut track = MixerTrack::new(
            source,
            args.volume,
            args.loops,
            played_frames.clone(),
            commands_rx,
            end_tx,
        );
        let fade_in_frames =
            (args.fade_in.as_secs_f64() * f64::from(self.sample_rate)).round() as u64;
        if fade_in_frames > 0 {
            track.volume = 0.0;
            track.amplitude = perceptual_level_to_amplitude(0.0);
            track.ramp = Some(VolumeRamp {
                from: 0.0,
                to: args.volume,
                start: 0,
                end: fade_in_frames,
            });
        }
        self.tracks.insert(handle, track);
        TrackHandle {
            id: handle,
            sample_rate: self.sample_rate,
            played_frames,
            commands: commands_tx,
            end_rx,
        }
    }

    /// Removes the track at once. Returns false if the track does not exist.
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
mod tests {
    use super::*;

    struct Constant;

    impl AudioSource for Constant {
        fn next_frame(&mut self) -> Option<[f32; 2]> {
            Some([1.0, 1.0])
        }
        fn seek(&mut self, _: Duration) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            Ok(())
        }
        fn duration(&self) -> Option<Duration> {
            None
        }
        fn reset(&mut self) {}
    }

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    fn mixer_with_track() -> (Mixer, TrackHandle) {
        let mut mixer = Mixer::new(1000);
        let handle = mixer.add_track(Box::new(Constant), MixerTrackArgs::default());
        (mixer, handle)
    }

    fn pull(mixer: &mut Mixer, frames: usize) -> Vec<f32> {
        mixer.drain_commands();
        (0..frames).map(|_| mixer.mix_next_frame()[0]).collect()
    }

    #[test]
    fn volume_ramp_started_late_ends_after_its_own_length() {
        let (mut mixer, handle) = mixer_with_track();
        pull(&mut mixer, 1000);
        handle
            .send(TrackCommand::SetVolume {
                volume: 0.0,
                fade: ms(1000),
                at: None,
            })
            .unwrap();
        let ramp = pull(&mut mixer, 1000);
        assert!(ramp[500] > 0.0 && ramp[500] < 1.0);
        assert_eq!(pull(&mut mixer, 1), [0.0]);
    }

    #[test]
    fn stop_fades_then_removes_the_track() {
        let (mut mixer, handle) = mixer_with_track();
        handle
            .send(TrackCommand::Stop {
                fade: ms(100),
                at: Some(ms(200)),
            })
            .unwrap();
        let out = pull(&mut mixer, 300);
        assert_eq!(out[150], 1.0);
        assert!(out[250] > 0.0 && out[250] < 1.0);
        assert!(!handle.has_ended());
        assert_eq!(pull(&mut mixer, 1), [0.0]);
        assert_eq!(handle.stop_reason(), Some(StopReason::Stopped));

        let (mut mixer, handle) = mixer_with_track();
        pull(&mut mixer, 500);
        handle
            .send(TrackCommand::Stop {
                fade: ms(100),
                at: Some(ms(100)),
            })
            .unwrap();
        let out = pull(&mut mixer, 100);
        assert!(out[50] > 0.0 && out[50] < 1.0);
        assert!(!handle.has_ended());
        pull(&mut mixer, 1);
        assert!(handle.has_ended());

        let (mut mixer, handle) = mixer_with_track();
        pull(&mut mixer, 10);
        handle
            .send(TrackCommand::Stop {
                fade: Duration::ZERO,
                at: None,
            })
            .unwrap();
        assert_eq!(pull(&mut mixer, 1), [0.0]);
        assert_eq!(handle.stop_reason(), Some(StopReason::Stopped));
        assert!(!handle.send(TrackCommand::SetLoops(true)).unwrap());
    }

    #[test]
    fn trigger_fires_at_its_frame_even_when_a_stop_lands_there() {
        let (mut mixer, handle) = mixer_with_track();
        let (trigger, mut rx) = TrackCommand::add_trigger(ms(100));
        handle.send(trigger).unwrap();
        pull(&mut mixer, 100);
        assert!(rx.try_recv().is_err());
        pull(&mut mixer, 1);
        assert!(rx.try_recv().is_ok());

        let (mut mixer, handle) = mixer_with_track();
        let (trigger, mut rx) = TrackCommand::add_trigger(ms(100));
        handle.send(trigger).unwrap();
        handle
            .send(TrackCommand::Stop {
                fade: Duration::ZERO,
                at: Some(ms(100)),
            })
            .unwrap();
        pull(&mut mixer, 101);
        assert!(rx.try_recv().is_ok());
        assert_eq!(handle.stop_reason(), Some(StopReason::Stopped));
    }
}
