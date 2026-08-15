use atomic_float::AtomicF32;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use crate::audio::helpers::perceptual_level_to_amplitude;

pub type TrackEndCallback = Arc<dyn Fn() + Send + Sync>;

pub trait AudioSource: Send + Sync {
    fn next_frame(&mut self) -> Option<[f32; 2]>;
    fn seek(&mut self, position: Duration) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
    #[allow(dead_code)]
    fn duration(&self) -> Option<Duration>;
    fn reset(&mut self);
}

pub struct MixerTrack {
    pub source: Box<dyn AudioSource>,
    pub volume: Arc<AtomicF32>,
    pub loops: bool,
    pub active: bool,
    pub end_callback: Option<TrackEndCallback>,
}

impl MixerTrack {
    pub fn new(source: Box<dyn AudioSource>, volume: f32, loops: bool) -> Self {
        // Convert user volume to amplitude using logarithmic scaling
        let amplitude = perceptual_level_to_amplitude(volume);
        Self {
            source,
            volume: Arc::new(AtomicF32::new(amplitude)),
            loops,
            active: true,
            end_callback: None,
        }
    }

    pub fn with_end_callback(mut self, callback: TrackEndCallback) -> Self {
        self.end_callback = Some(callback);
        self
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
                            callback();
                        }
                        return Some([0.0, 0.0]);
                    }
                }
            }
        }
    }
}

pub struct CustomMixer {
    tracks: HashMap<String, MixerTrack>,
    #[allow(dead_code)]
    sample_rate: u32,
    #[allow(dead_code)]
    channels: u16,
}

impl CustomMixer {
    pub fn new(sample_rate: u32) -> Self {
        Self {
            tracks: HashMap::new(),
            sample_rate,
            channels: 2,
        }
    }

    pub fn add_track_with_callback(
        &mut self,
        name: String,
        source: Box<dyn AudioSource>,
        volume: f32,
        loops: bool,
        callback: Option<TrackEndCallback>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if self.tracks.contains_key(&name) {
            return Err(format!("Track '{}' already exists", name).into());
        }

        let mut track = MixerTrack::new(source, volume, loops);
        if let Some(cb) = callback {
            track = track.with_end_callback(cb);
        }
        self.tracks.insert(name, track);

        Ok(())
    }

    pub fn remove_track(&mut self, name: &str) {
        self.tracks.remove(name);
    }

    #[allow(dead_code)]
    pub fn has_track(&self, name: &str) -> bool {
        self.tracks.contains_key(name)
    }

    /// Update the volume of a track.
    ///
    /// # Arguments
    ///
    /// * `name` - Name of the track to update
    /// * `volume` - User-facing volume (0.0-2.0). This is converted to amplitude using
    ///   perceptual (logarithmic) scaling. 0.0 = silence, 1.0 = unity gain, 2.0 = +18dB boost.
    ///
    /// # Returns
    ///
    /// Ok(()) if successful, or an error message if the track is not found.
    pub fn update_track_volume(&mut self, name: &str, volume: f32) -> Result<(), String> {
        let track = self
            .tracks
            .get(name)
            .ok_or_else(|| format!("Track '{}' not found", name))?;

        // Convert user volume to amplitude using logarithmic scaling
        let amplitude = perceptual_level_to_amplitude(volume);
        track.volume.store(amplitude, Ordering::Relaxed);
        Ok(())
    }

    pub fn update_track_loops(&mut self, name: &str, loops: bool) -> Result<(), String> {
        let track = self
            .tracks
            .get_mut(name)
            .ok_or_else(|| format!("Track '{}' not found", name))?;

        track.loops = loops;
        Ok(())
    }

    pub fn mix_next_frame(&mut self) -> [f32; 2] {
        let mut mixed = [0.0, 0.0];
        let mut inactive_tracks = Vec::new();

        for (name, track) in self.tracks.iter_mut() {
            if let Some(frame) = track.next_frame() {
                mixed[0] += frame[0];
                mixed[1] += frame[1];
            }

            if !track.active && !track.loops {
                inactive_tracks.push(name.clone());
            }
        }

        for name in inactive_tracks {
            self.tracks.remove(&name);
        }

        mixed[0] = mixed[0].clamp(-1.0, 1.0);
        mixed[1] = mixed[1].clamp(-1.0, 1.0);

        mixed
    }
}
