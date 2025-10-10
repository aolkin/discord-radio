use crate::state::Data;
use serenity::model::id::GuildId;
use songbird::Call;
use songbird::input::Input;
use songbird::tracks::TrackHandle;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
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
    pub handle: TrackHandle,
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
    call_lock: Arc<Mutex<Call>>,
    guild_id: GuildId,
    bot_state: Data,
}

impl TrackManager {
    pub fn new(call_lock: Arc<Mutex<Call>>, guild_id: GuildId, bot_state: Data) -> Self {
        Self {
            tracks: HashMap::new(),
            call_lock,
            guild_id,
            bot_state,
        }
    }

    pub async fn start_track(
        &mut self,
        args: StartTrackArgs,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.start_track_with_custom_handler::<crate::audio::events::TrackEndHandler>(args, None)
            .await
    }

    pub async fn start_track_with_custom_handler<H>(
        &mut self,
        args: StartTrackArgs,
        custom_handler: Option<H>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
    where
        H: songbird::events::EventHandler + 'static,
    {
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

        let handle = self
            .create_and_play_track_with_offset(&args.filename, args.start_position)
            .await;

        self.attach_handlers(&handle, &args.name, args.loops, custom_handler)?;

        let track = TrackInfo {
            name: args.name.clone(),
            filename: args.filename.clone(),
            handle,
            volume: args.volume,
            fade_task: None,
            fade_cancel: None,
            loops: args.loops,
            start_time: std::time::SystemTime::now(),
        };

        self.finalize_track_start(track, args.fade_time).await
    }

    async fn finalize_track_start(
        &mut self,
        mut track: TrackInfo,
        fade_time: f32,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Ensure start_time is at least set; callers are expected to set it.
        // Handle fade-in if requested, otherwise set volume directly.
        if fade_time > 0.0 {
            track.handle.set_volume(0.0)?;

            let fade_cancel = CancellationToken::new();
            let handle_clone = track.handle.clone();
            let cancel_clone = fade_cancel.clone();
            let target_volume = track.volume;

            let fade_task = tokio::spawn(async move {
                if let Err(e) = crate::audio::fade::fade_volume(
                    handle_clone,
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
            track.handle.set_volume(track.volume)?;
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

    async fn create_and_play_track_with_offset(
        &mut self,
        filename: &str,
        start_position: Option<Duration>,
    ) -> TrackHandle {
        #[allow(clippy::unnecessary_to_owned)]
        let source = Input::from(songbird::input::File::new(filename.to_string()));

        let handle = {
            let mut call = self.call_lock.lock().await;
            call.play_input(source)
        };

        if let Some(position) = start_position {
            tracing::info!("Seeking track to position {:.2}s", position.as_secs_f64());

            if let Err(e) = handle.seek_async(position).await {
                tracing::warn!(
                    "Failed to seek to position {:.2}s in {}: {}",
                    position.as_secs_f64(),
                    filename,
                    e
                );
            }
        }

        handle
    }

    fn attach_handlers<H>(
        &self,
        handle: &TrackHandle,
        name: &str,
        loops: bool,
        extra_end_handler: Option<H>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
    where
        H: songbird::events::EventHandler + 'static,
    {
        if loops {
            handle.enable_loop()?;

            let loop_handler = crate::audio::events::TrackLoopHandler::new(
                self.bot_state.clone(),
                self.guild_id,
                name.to_string(),
            );
            handle.add_event(
                songbird::Event::Track(songbird::events::TrackEvent::Loop),
                loop_handler,
            )?;
        } else {
            if let Some(h) = extra_end_handler {
                handle.add_event(songbird::Event::Track(songbird::events::TrackEvent::End), h)?;
            }

            let cleanup_handler = crate::audio::events::TrackEndHandler::new(
                self.bot_state.clone(),
                self.guild_id,
                name.to_string(),
            );
            handle.add_event(
                songbird::Event::Track(songbird::events::TrackEvent::End),
                cleanup_handler,
            )?;
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

        let handle = track_info.handle.clone();
        let current_volume = track_info.volume;
        track_info.volume = volume;

        if fade_time > 0.0 {
            let cancel_token = CancellationToken::new();
            let cancel_clone = cancel_token.clone();

            let fade_task = tokio::spawn(async move {
                if let Err(e) = crate::audio::fade::fade_volume(
                    handle,
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
            handle.set_volume(volume)?;
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

        if loops {
            track_info.handle.enable_loop()?;
        } else {
            track_info.handle.disable_loop()?;
        }

        self.persist_state().await;

        Ok(())
    }

    pub async fn remove_track(&mut self, name: &str) {
        if self.tracks.remove(name).is_some() {
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

        let handle = track_info.handle.clone();
        let current_volume = track_info.volume;

        if fade_time > 0.0 {
            let cancel_token = CancellationToken::new();
            let cancel_clone = cancel_token.clone();

            if let Err(e) = crate::audio::fade::fade_volume(
                handle.clone(),
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

        if let Err(e) = handle.stop() {
            tracing::warn!("Failed to stop track '{}': {}", name, e);
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
