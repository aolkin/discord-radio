use crate::audio::dj::config::DJConfig;
use crate::audio::dj::scheduler::{DJStateType, WeightedScheduler};
use crate::audio::tracks::{StartTrackArgs, TrackManager};
use crate::state::Data;
use rand::Rng;
use serenity::all::Http;
use serenity::model::id::{ChannelId, GuildId};
use std::sync::Arc;
use std::time::Duration;
use tracing::info;

#[derive(Clone, Debug)]
pub enum DJState {
    PlayingTrack {
        track_name: String,
        filename: String,
        started_at: std::time::Instant,
        duration: Duration,
        forced_profile: Option<String>,
        status_message: Option<String>, // The channel status pushed to status stack
    },
    PlayingHexMessage {
        message: String,
        started_at: std::time::Instant,
        target_loops: usize,
        forced_profile: Option<String>,
        status_message: Option<String>, // The obfuscated message pushed to status stack
    },
    PlayingNoise {
        noise_profile: String,
        started_at: std::time::Instant,
        duration: Duration,
    },
    Idle {
        started_at: std::time::Instant,
        duration: Duration,
    },
    Stopped,
}

impl DJState {
    pub fn is_complete(&self) -> bool {
        match self {
            DJState::PlayingTrack {
                started_at,
                duration,
                ..
            } => started_at.elapsed() >= *duration,
            DJState::PlayingHexMessage { .. } => false,
            DJState::PlayingNoise {
                started_at,
                duration,
                ..
            } => started_at.elapsed() >= *duration,
            DJState::Idle {
                started_at,
                duration,
            } => started_at.elapsed() >= *duration,
            DJState::Stopped => true,
        }
    }

    pub fn forced_profile(&self) -> Option<&str> {
        match self {
            DJState::PlayingTrack { forced_profile, .. } => forced_profile.as_deref(),
            DJState::PlayingHexMessage { forced_profile, .. } => forced_profile.as_deref(),
            DJState::PlayingNoise { noise_profile, .. } => Some(noise_profile.as_str()),
            _ => None,
        }
    }
}

pub struct DJStateMachine {
    current_state: DJState,
    scheduler: WeightedScheduler,
    guild_id: GuildId,
    content_path: String,
    announcement_channel: Option<ChannelId>,
    http: Arc<Http>,
    hex_message_announcements: Vec<String>,
}

impl DJStateMachine {
    pub fn new(
        config: DJConfig,
        guild_id: GuildId,
        content_path: String,
        announcement_channel: Option<ChannelId>,
        http: Arc<Http>,
        restored_state: Option<crate::persistence::DJStateMachineState>,
    ) -> Self {
        let current_state = if let Some(state) = restored_state {
            (&state).try_into().unwrap_or_else(|_| {
                tracing::warn!(
                    "Failed to restore DJ state for guild {}, starting from idle",
                    guild_id
                );
                DJState::Idle {
                    started_at: std::time::Instant::now(),
                    duration: Duration::from_secs(1),
                }
            })
        } else {
            DJState::Idle {
                started_at: std::time::Instant::now(),
                duration: Duration::from_secs(1),
            }
        };

        let hex_message_announcements =
            config.hex_message_announcements.clone().unwrap_or_default();

        Self {
            current_state,
            scheduler: WeightedScheduler::new(config),
            guild_id,
            content_path,
            announcement_channel,
            http,
            hex_message_announcements,
        }
    }

    pub fn current_state(&self) -> &DJState {
        &self.current_state
    }

    pub fn scheduler(&self) -> &WeightedScheduler {
        &self.scheduler
    }

    pub fn set_announcement_channel(&mut self, channel: Option<ChannelId>) {
        self.announcement_channel = channel;
    }

    pub async fn stop(&mut self, track_manager: &mut TrackManager, bot_state: &Data) {
        // Clean up current state before stopping
        if let Err(e) = self.cleanup_current_state(track_manager, bot_state).await {
            tracing::error!("Error cleaning up DJ state during stop: {}", e);
        }
        self.current_state = DJState::Stopped;
    }

    pub async fn force_advance(
        &mut self,
        track_manager: &mut TrackManager,
        bot_state: &Data,
        state_type_filter: Option<crate::audio::dj::manager::DJStateTypeFilter>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let next_state_type = if let Some(filter) = state_type_filter {
            self.scheduler.next_state_of_type(filter)
        } else {
            self.scheduler.next_state()
        };

        self.transition_to_state(next_state_type, track_manager, bot_state)
            .await
    }

    pub async fn force_hex_message(
        &mut self,
        track_manager: &mut TrackManager,
        bot_state: &Data,
        message: String,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.cleanup_current_state(track_manager, bot_state).await?;

        info!(
            "DJ force transitioning to hex message with custom text: {}",
            message
        );
        self.current_state = self
            .start_custom_hex_message_state(message, bot_state)
            .await?;

        Ok(())
    }

    pub async fn advance(
        &mut self,
        track_manager: &mut TrackManager,
        bot_state: &Data,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if !self.current_state.is_complete() {
            return Ok(());
        }

        let next_state_type = self.scheduler.next_state();
        self.transition_to_state(next_state_type, track_manager, bot_state)
            .await
    }

    async fn transition_to_state(
        &mut self,
        next_state_type: DJStateType,
        track_manager: &mut TrackManager,
        bot_state: &Data,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.cleanup_current_state(track_manager, bot_state).await?;

        info!("DJ transitioning to state: {:?}", next_state_type);
        self.current_state = self
            .create_next_state(next_state_type, track_manager, bot_state)
            .await?;

        Ok(())
    }

    async fn cleanup_current_state(
        &mut self,
        track_manager: &mut TrackManager,
        bot_state: &Data,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        match &self.current_state {
            DJState::PlayingTrack {
                track_name,
                status_message,
                ..
            } => {
                if track_manager.has_track(track_name) {
                    track_manager.stop_track(track_name, 1.0, false).await?;
                }

                // Remove the track status from the stack if present
                if let Some(status_msg) = status_message {
                    bot_state
                        .voice_status_manager
                        .remove_status(self.guild_id, status_msg, &self.http)
                        .await;
                }
            }
            DJState::PlayingHexMessage { status_message, .. } => {
                // Reset the hex playback state
                let hex_playback_states = bot_state.hex_playback_states.read().await;
                if let Some(state_arc) = hex_playback_states.get(&self.guild_id) {
                    let mut state = state_arc.write().await;
                    *state = crate::state::HexPlaybackState::stopped();
                }
                drop(hex_playback_states);

                // Remove the hex message status from the stack
                if let Some(status_msg) = status_message {
                    bot_state
                        .voice_status_manager
                        .remove_status(self.guild_id, status_msg, &self.http)
                        .await;
                }

                // Remove persisted message playback state
                if let Err(e) = bot_state
                    .state_store
                    .remove_message_playback(self.guild_id)
                    .await
                {
                    tracing::warn!("Failed to remove message playback state: {}", e);
                }
            }
            DJState::PlayingNoise { .. } => {
                // PlayingNoise doesn't play any tracks, it just forces a profile
                // No cleanup needed
            }
            _ => {}
        }

        Ok(())
    }

    async fn create_next_state(
        &mut self,
        state_type: DJStateType,
        track_manager: &mut TrackManager,
        bot_state: &Data,
    ) -> Result<DJState, Box<dyn std::error::Error + Send + Sync>> {
        match state_type {
            DJStateType::Track(idx) => self.start_track_state(idx, track_manager, bot_state).await,
            DJStateType::HexMessage(idx) => self.start_hex_message_state(idx, bot_state).await,
            DJStateType::Noise(idx) => self.start_noise_state(idx).await,
        }
    }

    async fn start_track_state(
        &mut self,
        idx: usize,
        track_manager: &mut TrackManager,
        bot_state: &Data,
    ) -> Result<DJState, Box<dyn std::error::Error + Send + Sync>> {
        let track_entry = self
            .scheduler
            .get_track(idx)
            .ok_or("Track index out of bounds")?;

        let full_path = format!("{}/{}", self.content_path, track_entry.filename);
        let track_name = format!("dj_track_{}", idx);

        let duration = bot_state.duration_cache.get_duration(&full_path).await;
        let (start_position, play_duration) = if track_entry.allow_subsection.unwrap_or(false) {
            if let Some(total_duration) = duration {
                if total_duration.as_secs_f32() > 240.0 {
                    let mut rng = rand::rng();
                    let subsection_duration =
                        Duration::from_secs_f32(rng.random_range(120.0..240.0));
                    let latest_start =
                        total_duration.as_secs_f32() - subsection_duration.as_secs_f32();
                    let start_secs = rng.random_range(0.0..latest_start.max(0.0));
                    (
                        Some(Duration::from_secs_f32(start_secs)),
                        subsection_duration,
                    )
                } else {
                    (None, duration.unwrap_or(Duration::from_secs(60)))
                }
            } else {
                (None, Duration::from_secs(60))
            }
        } else {
            let max_dur = track_entry
                .max_duration_seconds
                .map(Duration::from_secs_f32)
                .or(duration)
                .unwrap_or(Duration::from_secs(60));
            (None, max_dur)
        };

        track_manager
            .start_track(StartTrackArgs {
                name: track_name.clone(),
                filename: full_path,
                volume: track_entry.volume.unwrap_or(1.0),
                fade_time: 1.0,
                loops: false,
                start_position,
                persist: false,
            })
            .await?;

        tracing::info!(
            "DJ starting track '{}' (duration: {:.1}s) in guild {}",
            track_entry.filename,
            play_duration.as_secs_f32(),
            self.guild_id
        );

        // Push track channel status onto the voice channel status stack if configured
        let status_message = if let Some(ref status) = track_entry.channel_status {
            let status_with_emoji = format!("🔊 {}", status);
            bot_state
                .voice_status_manager
                .push_status(self.guild_id, status_with_emoji.clone(), &self.http)
                .await;
            Some(status_with_emoji)
        } else {
            None
        };

        Ok(DJState::PlayingTrack {
            track_name,
            filename: track_entry.filename.clone(),
            started_at: std::time::Instant::now(),
            duration: play_duration,
            forced_profile: track_entry.signal_profile.clone(),
            status_message,
        })
    }

    async fn start_hex_message_state(
        &mut self,
        idx: usize,
        bot_state: &Data,
    ) -> Result<DJState, Box<dyn std::error::Error + Send + Sync>> {
        let hex_entry = self
            .scheduler
            .get_hex_message(idx)
            .ok_or("Hex message index out of bounds")?;

        let loop_min = hex_entry
            .loop_min
            .unwrap_or(self.scheduler.config().hex_message_defaults.loop_min);
        let loop_max = hex_entry
            .loop_max
            .unwrap_or(self.scheduler.config().hex_message_defaults.loop_max);

        let forced_profile = hex_entry.signal_profile.clone().or(self
            .scheduler
            .config()
            .hex_message_defaults
            .signal_profile
            .clone());

        // Use custom announcement if present, otherwise choose randomly from defaults
        let announcement = if let Some(custom_announcement) = &hex_entry.announcement {
            Some(custom_announcement.clone())
        } else {
            self.pick_random_announcement()
        };

        self.play_hex_message(
            hex_entry.text.clone(),
            loop_min,
            loop_max,
            forced_profile,
            announcement,
            bot_state,
        )
        .await
    }

    async fn start_custom_hex_message_state(
        &mut self,
        message: String,
        bot_state: &Data,
    ) -> Result<DJState, Box<dyn std::error::Error + Send + Sync>> {
        // Use defaults from the DJ config
        let loop_min = self.scheduler.config().hex_message_defaults.loop_min;
        let loop_max = self.scheduler.config().hex_message_defaults.loop_max;
        let forced_profile = self
            .scheduler
            .config()
            .hex_message_defaults
            .signal_profile
            .clone();

        // Use random default announcement if available
        let announcement = self.pick_random_announcement();

        self.play_hex_message(
            message,
            loop_min,
            loop_max,
            forced_profile,
            announcement,
            bot_state,
        )
        .await
    }

    fn pick_random_announcement(&self) -> Option<String> {
        if !self.hex_message_announcements.is_empty() {
            let mut rng = rand::rng();
            let announcement_idx = rng.random_range(0..self.hex_message_announcements.len());
            Some(self.hex_message_announcements[announcement_idx].clone())
        } else {
            None
        }
    }

    async fn play_hex_message(
        &mut self,
        message: String,
        loop_min: u32,
        loop_max: u32,
        forced_profile: Option<String>,
        announcement: Option<String>,
        bot_state: &Data,
    ) -> Result<DJState, Box<dyn std::error::Error + Send + Sync>> {
        let target_loops = {
            let mut rng = rand::rng();
            if loop_min == loop_max {
                loop_min as usize
            } else {
                rng.random_range(loop_min..=loop_max) as usize
            }
        };

        let track_managers = bot_state.track_managers.read().await;
        let manager_arc = track_managers
            .get(&self.guild_id)
            .ok_or("Track manager not found for guild")?
            .clone();
        drop(track_managers);

        let hex_playback_state = bot_state
            .hex_playback_states
            .write()
            .await
            .entry(self.guild_id)
            .or_insert_with(|| {
                Arc::new(tokio::sync::RwLock::new(
                    crate::state::HexPlaybackState::stopped(),
                ))
            })
            .clone();

        self.ensure_hex_playback_task(bot_state, manager_arc.clone(), hex_playback_state.clone())
            .await;

        tracing::info!(
            "DJ playing hex message '{}' ({} loops) in guild {}",
            message,
            target_loops,
            self.guild_id
        );

        // Push hex message status onto the voice channel status stack
        let obfuscated = crate::commands::voice::obfuscate_message(&message);
        bot_state
            .voice_status_manager
            .push_status(self.guild_id, obfuscated.clone(), &self.http)
            .await;

        {
            let mut state = hex_playback_state.write().await;
            *state = crate::state::HexPlaybackState::playing(
                message.clone(),
                0,
                1.0,
                Some(target_loops),
                Some(obfuscated.clone()),
            );
        }

        // Send announcement to text channel if configured
        if let Some(channel_id) = self.announcement_channel
            && let Some(announcement_text) = announcement
        {
            let http_clone = self.http.clone();
            tokio::spawn(async move {
                if let Err(e) = channel_id.say(&http_clone, &announcement_text).await {
                    tracing::warn!("Failed to send DJ hex message announcement: {}", e);
                }
            });
        }

        Ok(DJState::PlayingHexMessage {
            message,
            started_at: std::time::Instant::now(),
            target_loops,
            forced_profile,
            status_message: Some(obfuscated),
        })
    }

    async fn ensure_hex_playback_task(
        &self,
        bot_state: &Data,
        manager_arc: Arc<tokio::sync::Mutex<TrackManager>>,
        playback_state: Arc<tokio::sync::RwLock<crate::state::HexPlaybackState>>,
    ) {
        let mut tasks = bot_state.hex_playback_tasks.write().await;
        if tasks.contains_key(&self.guild_id) {
            return;
        }

        let guild_id_copy = self.guild_id;
        let manager_copy = manager_arc.clone();
        let hex_audio_dir = format!("{}/audio/hex/", bot_state.content_path);
        let playback_state_copy = playback_state.clone();
        let bot_state_copy = bot_state.clone();

        let handle = tokio::spawn(async move {
            crate::audio::manager::hex_playback_task(
                guild_id_copy,
                manager_copy,
                hex_audio_dir,
                playback_state_copy,
                bot_state_copy,
            )
            .await;
        });

        tasks.insert(self.guild_id, handle);
    }

    async fn start_noise_state(
        &mut self,
        idx: usize,
    ) -> Result<DJState, Box<dyn std::error::Error + Send + Sync>> {
        let noise_entry = self
            .scheduler
            .get_noise_period(idx)
            .ok_or("Noise period index out of bounds")?;

        let duration_secs = {
            let mut rng = rand::rng();
            rng.random_range(noise_entry.min_duration_seconds..noise_entry.max_duration_seconds)
        };
        let duration = Duration::from_secs_f32(duration_secs);

        let noise_profile = noise_entry.noise_profile.clone();

        tracing::info!(
            "DJ playing noise with profile '{}' for {:.1}s in guild {}",
            noise_profile,
            duration_secs,
            self.guild_id
        );

        Ok(DJState::PlayingNoise {
            noise_profile,
            started_at: std::time::Instant::now(),
            duration,
        })
    }
}
