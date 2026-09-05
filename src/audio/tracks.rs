use crate::audio::custom_mixer::AudioSource;
use crate::audio::decoder::SymphoniaSource;
use crate::audio::processing_thread::AudioProcessor;
use crate::state::Data;
use serenity::model::id::GuildId;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

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
    pub volume: f32,
    pub fade_task: Option<JoinHandle<()>>,
    pub fade_cancel: Option<CancellationToken>,
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
        callback: Option<crate::audio::custom_mixer::TrackEndCallback>,
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
        {
            let mut processor = processor_arc.write().await;

            // Create cleanup callback that removes track from TrackManager when it ends
            // Only for non-looping tracks
            // Note: Callbacks are invoked from Songbird's audio thread (no tokio runtime)
            // We use std::thread::spawn with a runtime handle to bridge to async
            let final_callback = if !args.loops {
                let guild_id = self.guild_id;
                let bot_state = self.bot_state.clone();
                let track_name_for_cleanup = args.name.clone();

                let cleanup_cb: crate::audio::custom_mixer::TrackEndCallback =
                    Arc::new(move || {
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
                                    manager.remove_track(&name_copy).await;
                                }
                            });
                        });
                    });

                // Combine with user callback if provided
                if let Some(user_cb) = callback {
                    Some(Arc::new(move || {
                        user_cb();
                        cleanup_cb();
                    })
                        as crate::audio::custom_mixer::TrackEndCallback)
                } else {
                    Some(cleanup_cb)
                }
            } else {
                // Looping tracks: just use user callback if provided
                callback
            };

            processor.mixer_mut().add_track_with_callback(
                args.name.clone(),
                Box::new(source),
                args.volume,
                args.loops,
                final_callback,
            )?;
        }

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
            volume: args.volume,
            fade_task: None,
            fade_cancel: None,
            loops: args.loops,
            start_time,
            persist: args.persist,
        };

        self.finalize_track_start_dsp(track, args.fade_time, processor_arc.clone())
            .await
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
            proc.mixer_mut()
                .update_track_volume(&track.name, track.volume)?;
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

        if let Some(cancel_token) = &track_info.fade_cancel {
            cancel_token.cancel();
        }

        let current_volume = track_info.volume;
        track_info.volume = volume;

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
        proc.mixer_mut().update_track_loops(name, loops)?;

        self.persist_state().await;

        Ok(())
    }

    pub async fn remove_track(&mut self, name: &str) {
        if self.tracks.remove(name).is_some() {
            // Also remove from mixer
            let mut proc = self.audio_processor.write().await;
            proc.mixer_mut().remove_track(name);

            tracing::debug!("Removed track '{}' from guild {}", name, self.guild_id);
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

        if fade_time > 0.0 {
            let cancel_token = CancellationToken::new();
            let cancel_clone = cancel_token.clone();
            let processor_clone = self.audio_processor.clone();
            let track_name = name.to_string();

            // Spawn fade in background - don't block!
            tokio::spawn(async move {
                if let Err(e) = fade_volume_dsp(
                    processor_clone.clone(),
                    track_name.clone(),
                    current_volume,
                    0.0,
                    fade_time,
                    cancel_clone.clone(),
                )
                .await
                {
                    tracing::warn!("Fade out failed for track '{}': {}", track_name, e);
                }

                // Remove from mixer after fade completes
                let mut proc = processor_clone.write().await;
                proc.mixer_mut().remove_track(&track_name);
            });
        } else {
            // No fade - remove immediately
            let mut proc = self.audio_processor.write().await;
            proc.mixer_mut().remove_track(name);
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

/// Fade volume from one level to another over time.
///
/// This function interpolates linearly in user volume space (0.0-2.0), then converts
/// each intermediate value to amplitude using perceptual scaling. The conversion to
/// amplitude happens in `update_track_volume`.
///
/// # Arguments
///
/// * `processor` - Audio processor containing the mixer
/// * `track_name` - Name of the track to fade
/// * `from_volume` - Starting user volume (0.0-2.0)
/// * `to_volume` - Target user volume (0.0-2.0)
/// * `fade_time` - Duration of fade in seconds
/// * `cancel` - Token to cancel the fade early
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
        // Interpolate in user volume space
        let current_volume = from_volume + (to_volume - from_volume) * progress;

        {
            let mut proc = processor.write().await;
            if proc
                .mixer_mut()
                .update_track_volume(&track_name, current_volume)
                .is_err()
            {
                // Track might have been removed
                return Ok(());
            }
        }

        tokio::time::sleep(step_duration).await;
    }

    Ok(())
}

/// Get or create a TrackManager for a guild, ensuring audio processor exists
pub async fn get_or_create_track_manager(
    bot_state: &Data,
    guild_id: GuildId,
) -> Arc<tokio::sync::Mutex<TrackManager>> {
    // Ensure audio processor exists for this guild
    let processor = get_or_create_audio_processor(bot_state, guild_id).await;

    let mut track_managers = bot_state.track_managers.write().await;

    // Check if manager exists, if not create it with the processor
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
