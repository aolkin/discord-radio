use atomic_float::AtomicF32;
use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

pub trait AudioSource: Send + Sync {
    fn next_frame(&mut self) -> Option<[f32; 2]>;
    fn seek(&mut self, position: Duration) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
    fn duration(&self) -> Option<Duration>;
    fn reset(&mut self);
}

pub struct MixerTrack {
    pub source: Box<dyn AudioSource>,
    pub volume: Arc<AtomicF32>,
    pub loops: bool,
    pub active: bool,
}

impl MixerTrack {
    pub fn new(source: Box<dyn AudioSource>, volume: f32, loops: bool) -> Self {
        Self {
            source,
            volume: Arc::new(AtomicF32::new(volume)),
            loops,
            active: true,
        }
    }

    pub fn next_frame(&mut self) -> Option<[f32; 2]> {
        if !self.active {
            return Some([0.0, 0.0]);
        }

        match self.source.next_frame() {
            Some(frame) => {
                let vol = self.volume.load(Ordering::Relaxed);
                Some([frame[0] * vol, frame[1] * vol])
            }
            None => {
                if self.loops {
                    self.source.reset();
                    self.next_frame()
                } else {
                    self.active = false;
                    Some([0.0, 0.0])
                }
            }
        }
    }
}

pub struct CustomMixer {
    tracks: HashMap<String, MixerTrack>,
    sample_rate: u32,
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

    pub fn add_track(
        &mut self,
        name: String,
        source: Box<dyn AudioSource>,
        volume: f32,
        loops: bool,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if self.tracks.contains_key(&name) {
            return Err(format!("Track '{}' already exists", name).into());
        }

        let track = MixerTrack::new(source, volume, loops);
        self.tracks.insert(name, track);

        Ok(())
    }

    pub fn remove_track(&mut self, name: &str) {
        self.tracks.remove(name);
    }

    pub fn has_track(&self, name: &str) -> bool {
        self.tracks.contains_key(name)
    }

    pub fn update_track_volume(&mut self, name: &str, volume: f32) -> Result<(), String> {
        let track = self
            .tracks
            .get(name)
            .ok_or_else(|| format!("Track '{}' not found", name))?;

        track.volume.store(volume, Ordering::Relaxed);
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

    pub fn fill_buffer(&mut self, frame_count: usize) -> Vec<[f32; 2]> {
        (0..frame_count).map(|_| self.mix_next_frame()).collect()
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn channels(&self) -> u16 {
        self.channels
    }
}
