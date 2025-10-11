use crate::audio::custom_mixer::AudioSource;
use crate::audio::decoder::SymphoniaSource;
use crate::audio::processing_thread::AudioProcessor;
use crate::state::Data;
use serenity::model::id::GuildId;
use songbird::Call;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, RwLock};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

#[derive(Clone, Debug)]
pub struct TrackSnapshot {
    pub name: String,
    pub filename: String,
    pub volume: f32,
    pub loops: bool,
}

pub struct TrackInfo {
    pub name: String,
    pub filename: String,
    pub volume: f32,
    pub fade_task: Option<JoinHandle<()>>,
    pub fade_cancel: Option<CancellationToken>,
    pub loops: bool,
    pub start_time: std::time::SystemTime,
}

#[derive(Clone, Debug)]
pub struct StartTrackArgs {
    pub name: String,
    pub filename: String,
    pub volume: f32,
    pub fade_time: f32,
    pub loops: bool,
    pub start_position: Option<Duration>,
}

pub struct TrackManager {
    tracks: HashMap<String, TrackInfo>,
    guild_id: GuildId,
    bot_state: Data,
    audio_processor: Option<Arc<RwLock<AudioProcessor>>>,
}

impl TrackManager {
    pub fn new(_call_lock: Arc<Mutex<Call>>, guild_id: GuildId, bot_state: Data) -> Self {
        Self {
            tracks: HashMap::new(),
            guild_id,
            bot_state,
            audio_processor: None,
        }
    }

    pub fn set_audio_processor(&mut self, processor: Arc<RwLock<AudioProcessor>>) {
        self.audio_processor = Some(processor);
    }

    pub async fn start_track(
        &mut self,
        args: StartTrackArgs,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.start_track_with_callback(args, None).await
    }

    pub async fn start_track_with_callback(
        &mut self,
        args: StartTrackArgs,
        callback: Option<crate::audio::custom_mixer::TrackEndCallback>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.validate_and_prepare_track(&args.name, &args.filename)
            .await?;
        let duration = self
            .bot_state
            .duration_cache
            .get_duration(&args.filename)
            .await;

        let position_info = if let Some(pos) = args.start_position {
            format!(", starting at: {:.2}s", pos.as_secs_f64())
        } else {
            String::new()
        };

        tracing::info!(
            "Starting track '{}' (file: {}, duration: {:.2}s, volume: {}, fade: {}s, loops: {}{}) in guild {}",
            args.name,
            args.filename,
            duration.map(|d| d.as_secs_f64()).unwrap_or(-1.0),
            args.volume,
            args.fade_time,
            args.loops,
            position_info,
            self.guild_id
        );

        // Create audio source through DSP pipeline if processor available
        if let Some(processor_arc) = &self.audio_processor {
            let mut source = SymphoniaSource::from_file(&args.filename, 48000)?;

            // Seek to start position if specified
            if let Some(pos) = args.start_position {
                source.seek(pos)?;
            }

            // Add to mixer
            {
                let mut processor = processor_arc.write().await;
                if let Some(cb) = callback {
                    processor.mixer_mut().add_track_with_callback(
                        args.name.clone(),
                        Box::new(source),
                        args.volume,
                        args.loops,
                        Some(cb),
                    )?;
                } else {
                    processor.mixer_mut().add_track(
                        args.name.clone(),
                        Box::new(source),
                        args.volume,
                        args.loops,
                    )?;
                }
            }

            let track = TrackInfo {
                name: args.name.clone(),
                filename: args.filename.clone(),
                volume: args.volume,
                fade_task: None,
                fade_cancel: None,
                loops: args.loops,
                start_time: std::time::SystemTime::now(),
            };

            self.finalize_track_start_dsp(track, args.fade_time, processor_arc.clone()).await
        } else {
            // Fallback: use Songbird directly (should not happen in normal operation)
            tracing::warn!("Audio processor not available for guild {}, track won't have DSP effects", self.guild_id);
            Err("Audio processor not initialized".into())
        }
    }

    async fn finalize_track_start_dsp(
        &mut self,
        mut track: TrackInfo,
        fade_time: f32,
        processor: Arc<RwLock<AudioProcessor>>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if fade_time > 0.0 {
            // Set initial volume to 0 in mixer
            {
                let mut proc = processor.write().await;
                proc.mixer_mut().update_track_volume(&track.name, 0.0)?;
            }

            let fade_cancel = CancellationToken::new();
            let processor_clone = processor.clone();
            let cancel_clone = fade_cancel.clone();
            let target_volume = track.volume;
            let track_name = track.name.clone();

            let fade_task = tokio::spawn(async move {
                if let Err(e) = fade_volume_dsp(
                    processor_clone,
                    track_name,
                    0.0,
                    target_volume,
                    fade_time,
                    cancel_clone,
                )
                .await
                {
                    tracing::warn!("Fade in failed: {}", e);
                }
            });

            track.fade_task = Some(fade_task);
            track.fade_cancel = Some(fade_cancel);
        } else {
            // Set volume directly in mixer
            let mut proc = processor.write().await;
            proc.mixer_mut().update_track_volume(&track.name, track.volume)?;
        }

        let key = track.name.clone();
        self.tracks.insert(key, track);

        self.persist_state().await;

        Ok(())
    }


    async fn validate_and_prepare_track(
        &self,
        name: &str,
        filename: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if self.tracks.contains_key(name) {
            return Err(format!("Track '{}' already exists", name).into());
        }

        if !Path::new(filename).exists() {
            return Err(format!("Audio file not found: {}", filename).into());
        }

        Ok(())
    }

    pub fn has_track(&self, name: &str) -> bool {
        self.tracks.contains_key(name)
    }

    pub async fn update_track_volume(
        &mut self,
        name: &str,
        volume: f32,
        fade_time: f32,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let track_info = self
            .tracks
            .get_mut(name)
            .ok_or_else(|| format!("Track '{}' not found", name))?;

        tracing::info!(
            "Updating track '{}' volume from {} to {} (fade: {}s) in guild {}",
            name,
            track_info.volume,
            volume,
            fade_time,
            self.guild_id
        );

        if let Some(cancel_token) = &track_info.fade_cancel {
            cancel_token.cancel();
        }

        let current_volume = track_info.volume;
        track_info.volume = volume;

        if let Some(processor_arc) = &self.audio_processor {
            if fade_time > 0.0 {
                let cancel_token = CancellationToken::new();
                let cancel_clone = cancel_token.clone();
                let processor_clone = processor_arc.clone();
                let track_name = name.to_string();

                let fade_task = tokio::spawn(async move {
                    if let Err(e) = fade_volume_dsp(
                        processor_clone,
                        track_name,
                        current_volume,
                        volume,
                        fade_time,
                        cancel_clone,
                    )
                    .await
                    {
                        tracing::warn!("Volume fade failed: {}", e);
                    }
                });

                track_info.fade_task = Some(fade_task);
                track_info.fade_cancel = Some(cancel_token);
            } else {
                let mut proc = processor_arc.write().await;
                proc.mixer_mut().update_track_volume(name, volume)?;
                track_info.fade_task = None;
                track_info.fade_cancel = None;
            }
        } else {
            return Err("Audio processor not initialized".into());
        }

        self.persist_state().await;

        Ok(())
    }

    pub async fn update_track_loops(
        &mut self,
        name: &str,
        loops: bool,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let track_info = self
            .tracks
            .get_mut(name)
            .ok_or_else(|| format!("Track '{}' not found", name))?;

        if track_info.loops == loops {
            return Ok(());
        }

        tracing::info!(
            "Updating track '{}' loops from {} to {} in guild {}",
            name,
            track_info.loops,
            loops,
            self.guild_id
        );

        track_info.loops = loops;

        if let Some(processor_arc) = &self.audio_processor {
            let mut proc = processor_arc.write().await;
            proc.mixer_mut().update_track_loops(name, loops)?;
        } else {
            return Err("Audio processor not initialized".into());
        }

        self.persist_state().await;

        Ok(())
    }

    pub async fn remove_track(&mut self, name: &str) {
        if self.tracks.remove(name).is_some() {
            // Also remove from mixer
            if let Some(processor_arc) = &self.audio_processor {
                let mut proc = processor_arc.write().await;
                proc.mixer_mut().remove_track(name);
            }

            tracing::debug!("Removed track '{}' from guild {}", name, self.guild_id);
            self.persist_state().await;
        }
    }

    pub async fn update_track_start_time(&mut self, name: &str) {
        if let Some(track_info) = self.tracks.get_mut(name) {
            track_info.start_time = std::time::SystemTime::now();
            self.persist_state().await;
        }
    }

    pub async fn stop_track(
        &mut self,
        name: &str,
        fade_time: f32,
        persist: bool,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let track_info = self
            .tracks
            .get(name)
            .ok_or_else(|| format!("Track '{}' not found", name))?;

        tracing::info!(
            "Stopping track '{}' (fade: {}s) in guild {}",
            name,
            fade_time,
            self.guild_id
        );

        if let Some(cancel_token) = &track_info.fade_cancel {
            cancel_token.cancel();
        }

        let current_volume = track_info.volume;

        if let Some(processor_arc) = &self.audio_processor {
            if fade_time > 0.0 {
                let cancel_token = CancellationToken::new();
                let cancel_clone = cancel_token.clone();
                let processor_clone = processor_arc.clone();
                let track_name = name.to_string();

                if let Err(e) = fade_volume_dsp(
                    processor_clone,
                    track_name,
                    current_volume,
                    0.0,
                    fade_time,
                    cancel_clone,
                )
                .await
                {
                    tracing::warn!("Fade out failed for track '{}': {}", name, e);
                }
            }

            // Remove from mixer
            let mut proc = processor_arc.write().await;
            proc.mixer_mut().remove_track(name);
        }

        self.tracks.remove(name);
        if persist {
            self.persist_state().await;
        }

        Ok(())
    }

    pub fn get_all_tracks(&self) -> Vec<TrackSnapshot> {
        self.tracks
            .values()
            .map(|info| TrackSnapshot {
                name: info.name.clone(),
                filename: info.filename.clone(),
                volume: info.volume,
                loops: info.loops,
            })
            .collect()
    }

    pub async fn stop_all_tracks(
        &mut self,
        fade_time: f32,
        persist: bool,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let track_names: Vec<String> = self.tracks.keys().cloned().collect();

        for name in track_names {
            if let Err(e) = self.stop_track(&name, fade_time, persist).await {
                tracing::warn!("Failed to stop track {}: {}", name, e);
            }
        }

        Ok(())
    }

    async fn persist_state(&self) {
        let state = crate::persistence::MultiTrackPlaybackState {
            tracks: self
                .tracks
                .values()
                .map(|info| crate::persistence::TrackState {
                    name: info.name.clone(),
                    filename: info.filename.clone(),
                    volume: info.volume,
                    loops: info.loops,
                    start_time: Some(info.start_time),
                })
                .collect(),
        };

        if let Err(e) = self
            .bot_state
            .state_store
            .save_multitrack_playback(self.guild_id, &state)
            .await
        {
            tracing::warn!("Failed to persist multitrack state: {}", e);
        }
    }
}

async fn fade_volume_dsp(
    processor: Arc<RwLock<AudioProcessor>>,
    track_name: String,
    from_volume: f32,
    to_volume: f32,
    fade_time: f32,
    cancel: CancellationToken,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let steps = (fade_time * 50.0) as u32;
    let step_duration = Duration::from_millis((fade_time * 1000.0 / steps as f32) as u64);

    for i in 0..=steps {
        if cancel.is_cancelled() {
            return Ok(());
        }

        let progress = i as f32 / steps as f32;
        let current_volume = from_volume + (to_volume - from_volume) * progress;

        {
            let mut proc = processor.write().await;
            if let Err(_) = proc.mixer_mut().update_track_volume(&track_name, current_volume) {
                // Track might have been removed
                return Ok(());
            }
        }

        tokio::time::sleep(step_duration).await;
    }

    Ok(())
}
