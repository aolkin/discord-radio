use crate::audio::custom_mixer::{AudioSource, TrackEndCallback, TrackHandle};
use crate::audio::decoder::SymphoniaSource;
use crate::audio::processing_thread::AudioProcessor;
use crate::state::Data;
use serenity::model::id::GuildId;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

#[derive(Clone, Debug)]
pub struct TrackSnapshot {
    pub name: String,
    pub filename: String,
    pub volume: f32,
    pub loops: bool,
    pub start_time: std::time::SystemTime,
    pub duration: Option<Duration>,
}

pub struct TrackInfo {
    pub name: String,
    /// The raw filename or `s3://` key, not a resolved path — persisting the
    /// resolved form would break restoration after an R2 cache eviction.
    pub filename: String,
    pub mixer_handle: TrackHandle,
    pub volume: f32,
    pub loops: bool,
    pub start_time: std::time::SystemTime,
    pub persist: bool,
}

#[derive(Clone, Debug)]
pub struct StartTrackArgs {
    pub name: String,
    pub filename: String,
    pub volume: f32,
    pub fade_time: f32,
    pub loops: bool,
    pub start_position: Option<Duration>,
    pub persist: bool,
}

pub struct TrackManager {
    tracks: HashMap<String, TrackInfo>,
    guild_id: GuildId,
    bot_state: Data,
    audio_processor: Arc<RwLock<AudioProcessor>>,
}

impl TrackManager {
    pub fn new(guild_id: GuildId, bot_state: Data, processor: Arc<RwLock<AudioProcessor>>) -> Self {
        Self {
            tracks: HashMap::new(),
            guild_id,
            bot_state,
            audio_processor: processor,
        }
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
        callback: Option<TrackEndCallback>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let resolved_filename = self.bot_state.file_resolver.resolve(&args.filename).await?;

        self.validate_and_prepare_track(&args.name, &resolved_filename)
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
            resolved_filename,
            duration.map(|d| d.as_secs_f64()).unwrap_or(-1.0),
            args.volume,
            args.fade_time,
            args.loops,
            position_info,
            self.guild_id
        );

        // Create audio source through DSP pipeline
        let processor_arc = self.audio_processor.clone();
        let mut source = SymphoniaSource::from_file(&resolved_filename, 48000)?;

        // Seek to start position if specified
        if let Some(pos) = args.start_position {
            source.seek(pos)?;
        }

        // Add to mixer with automatic cleanup callback
        let mixer_handle = {
            let mut processor = processor_arc.write().await;

            // Create cleanup callback that removes track from TrackManager when it ends
            // Only for non-looping tracks
            // Note: Callbacks are invoked from Songbird's audio thread (no tokio runtime)
            // We use std::thread::spawn with a runtime handle to bridge to async
            let final_callback = if !args.loops {
                let guild_id = self.guild_id;
                let bot_state = self.bot_state.clone();
                let track_name_for_cleanup = args.name.clone();

                let cleanup_cb: TrackEndCallback = Arc::new(move |handle| {
                    let guild_id_copy = guild_id;
                    let bot_state_copy = bot_state.clone();
                    let name_copy = track_name_for_cleanup.clone();

                    // Spawn a std::thread that creates its own tokio runtime
                    std::thread::spawn(move || {
                        let rt = tokio::runtime::Runtime::new().unwrap();
                        rt.block_on(async move {
                            tracing::info!(
                                "Track '{}' finished in guild {}, cleaning up",
                                name_copy,
                                guild_id_copy
                            );
                            let track_managers = bot_state_copy.track_managers.read().await;
                            if let Some(manager_arc) = track_managers.get(&guild_id_copy) {
                                let mut manager = manager_arc.lock().await;
                                manager.remove_handle(handle).await;
                            }
                        });
                    });
                });

                // Combine with user callback if provided
                if let Some(user_cb) = callback {
                    Some(Arc::new(move |handle| {
                        user_cb(handle);
                        cleanup_cb(handle);
                    }) as TrackEndCallback)
                } else {
                    Some(cleanup_cb)
                }
            } else {
                // Looping tracks: just use user callback if provided
                callback
            };

            let mixer = processor.mixer_mut();
            if args.fade_time > 0.0 {
                let handle = mixer.add_track(Box::new(source), 0.0, args.loops, final_callback);
                mixer.ramp_track_volume(
                    handle,
                    args.volume,
                    Duration::from_secs_f32(args.fade_time),
                )?;
                handle
            } else {
                mixer.add_track(Box::new(source), args.volume, args.loops, final_callback)
            }
        };

        // If we're starting from a position other than the beginning, adjust start_time
        // backwards so that elapsed time calculations include the seek position
        let start_time = if let Some(pos) = args.start_position {
            std::time::SystemTime::now() - pos
        } else {
            std::time::SystemTime::now()
        };

        let track = TrackInfo {
            name: args.name.clone(),
            filename: args.filename.clone(),
            mixer_handle,
            volume: args.volume,
            loops: args.loops,
            start_time,
            persist: args.persist,
        };

        self.tracks.insert(args.name, track);

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
        // Get processor first to avoid borrowing issues
        let processor_arc = self.audio_processor.clone();

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

        track_info.volume = volume;

        {
            let mut proc = processor_arc.write().await;
            let mixer = proc.mixer_mut();
            if fade_time > 0.0 {
                mixer.ramp_track_volume(
                    track_info.mixer_handle,
                    volume,
                    Duration::from_secs_f32(fade_time),
                )?;
            } else {
                mixer.update_track_volume(track_info.mixer_handle, volume)?;
            }
        }

        self.persist_state().await;

        Ok(())
    }

    pub async fn update_track_loops(
        &mut self,
        name: &str,
        loops: bool,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Get processor first to avoid borrowing issues
        let processor_arc = self.audio_processor.clone();

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

        let mut proc = processor_arc.write().await;
        proc.mixer_mut()
            .update_track_loops(track_info.mixer_handle, loops)?;

        self.persist_state().await;

        Ok(())
    }

    pub async fn remove_track(&mut self, name: &str) {
        let info = self.tracks.remove(name);
        self.finish_removing_track(info).await;
    }

    async fn remove_handle(&mut self, handle: TrackHandle) {
        let info = self
            .tracks
            .extract_if(|_k, info| info.mixer_handle == handle)
            .map(|(_, info)| info)
            .next();
        self.finish_removing_track(info).await;
    }

    async fn finish_removing_track(&mut self, info: Option<TrackInfo>) {
        if let Some(info) = info {
            let mut proc = self.audio_processor.write().await;
            proc.mixer_mut().remove_track(info.mixer_handle);

            tracing::debug!("Removed track '{}' from guild {}", info.name, self.guild_id);
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

        let handle = track_info.mixer_handle;

        {
            let mut proc = self.audio_processor.write().await;
            let mixer = proc.mixer_mut();
            if fade_time > 0.0 {
                mixer.fade_out_and_remove_track(handle, Duration::from_secs_f32(fade_time));
            } else {
                mixer.remove_track(handle);
            }
        }

        self.tracks.remove(name);
        if persist {
            self.persist_state().await;
        }

        Ok(())
    }

    pub async fn get_all_tracks(&self) -> Vec<TrackSnapshot> {
        let mut snapshots = Vec::new();
        for info in self.tracks.values() {
            let duration = self
                .bot_state
                .duration_cache
                .get_duration(&info.filename)
                .await;
            snapshots.push(TrackSnapshot {
                name: info.name.clone(),
                filename: info.filename.clone(),
                volume: info.volume,
                loops: info.loops,
                start_time: info.start_time,
                duration,
            });
        }
        snapshots
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
                .filter(|info| info.persist)
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

/// Get or create a TrackManager for a guild, ensuring audio processor exists
pub async fn get_or_create_track_manager(
    bot_state: &Data,
    guild_id: GuildId,
) -> Arc<tokio::sync::Mutex<TrackManager>> {
    if let Some(manager) = bot_state.track_managers.read().await.get(&guild_id) {
        return manager.clone();
    }

    let processor = get_or_create_audio_processor(bot_state, guild_id).await;

    let mut track_managers = bot_state.track_managers.write().await;
    track_managers
        .entry(guild_id)
        .or_insert_with(|| {
            let manager = TrackManager::new(guild_id, bot_state.clone(), processor);
            Arc::new(tokio::sync::Mutex::new(manager))
        })
        .clone()
}

/// Get or create audio processor for a guild
pub async fn get_or_create_audio_processor(
    bot_state: &Data,
    guild_id: GuildId,
) -> Arc<RwLock<AudioProcessor>> {
    // Try to get existing processor
    {
        let processors = bot_state.audio_processors.read().await;
        if let Some(processor) = processors.get(&guild_id) {
            return processor.clone();
        }
    }

    // Create new processor with default profile
    tracing::debug!(
        "Creating audio processor for guild {} (no voice connection yet)",
        guild_id
    );

    let initial_profile = load_default_profile(bot_state);
    let processor = Arc::new(RwLock::new(AudioProcessor::new(initial_profile)));

    // Store the processor
    let mut processors = bot_state.audio_processors.write().await;
    // Double-check in case another task created it while we were waiting
    processors
        .entry(guild_id)
        .or_insert_with(|| processor.clone());

    processor
}

/// Load default profile, using fallback if not found
fn load_default_profile(bot_state: &Data) -> crate::audio::profiles::SignalProfile {
    // Try to get the "clear" profile
    if let Some(profile) = bot_state.profile_manager.get_profile("clear").cloned() {
        return profile;
    }

    // Fallback: create a basic clear profile
    tracing::debug!("Default 'clear' profile not found, using fallback");
    crate::audio::profiles::SignalProfile {
        name: "clear".to_string(),
        bandpass_low: 20.0,
        bandpass_high: 20000.0,
        white_noise_level: 0.0,
        pink_noise_level: 0.0,
        brown_noise_level: 0.0,
        tremolo_depth: 0.0,
        tremolo_rate: 0.0,
        tremolo_jitter: 0.0,
        clip_pregain: 1.0,
        clip_threshold: 1.0,
        bitcrush_bits: None,
        dropout_probability: 0.0,
        dropout_duration_ms: (0.0, 0.0),
        frequency_warble_hz: None,
    }
}
