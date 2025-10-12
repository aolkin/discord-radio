use crate::audio::dj::config::{DJConfig, NoiseTypeConfig};
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
    },
    PlayingHexMessage {
        message: String,
        started_at: std::time::Instant,
    },
    PlayingNoise {
        noise_type: String,
        started_at: std::time::Instant,
        duration: Duration,
    },
    TransitioningProfile {
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
            DJState::TransitioningProfile {
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

    pub async fn stop(&mut self, track_manager: &mut TrackManager) {
        // Clean up current state before stopping
        if let Err(e) = self.cleanup_current_state(track_manager).await {
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
        self.cleanup_current_state(track_manager).await?;

        info!("DJ transitioning to state: {:?}", next_state_type);
        self.current_state = self
            .create_next_state(next_state_type, track_manager, bot_state)
            .await?;

        Ok(())
    }

    async fn cleanup_current_state(
        &mut self,
        track_manager: &mut TrackManager,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        match &self.current_state {
            DJState::PlayingTrack { track_name, .. } => {
                if track_manager.has_track(track_name) {
                    track_manager.stop_track(track_name, 1.0, false).await?;
                }
            }
            DJState::PlayingNoise { noise_type, .. } => {
                let track_name = format!("dj_noise_{}", noise_type);
                if track_manager.has_track(&track_name) {
                    track_manager.stop_track(&track_name, 1.0, false).await?;
                }
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
            DJStateType::Noise(idx) => self.start_noise_state(idx, track_manager).await,
            DJStateType::ProfileChange(idx) => self.start_profile_state(idx, bot_state).await,
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
                volume: 1.0,
                fade_time: 1.0,
                loops: false,
                start_position,
            })
            .await?;

        tracing::info!(
            "DJ starting track '{}' (duration: {:.1}s) in guild {}",
            track_entry.filename,
            play_duration.as_secs_f32(),
            self.guild_id
        );

        Ok(DJState::PlayingTrack {
            track_name,
            filename: track_entry.filename.clone(),
            started_at: std::time::Instant::now(),
            duration: play_duration,
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

        {
            let mut state = hex_playback_state.write().await;
            *state = crate::state::HexPlaybackState::playing(hex_entry.text.clone(), 0, 1.0);
        }

        tracing::info!(
            "DJ playing hex message '{}' in guild {}",
            hex_entry.text,
            self.guild_id
        );

        // Send announcement to text channel if configured
        if let Some(channel_id) = self.announcement_channel
            && !self.hex_message_announcements.is_empty()
        {
            let mut rng = rand::rng();
            let announcement_idx = rng.random_range(0..self.hex_message_announcements.len());
            let announcement = &self.hex_message_announcements[announcement_idx];

            let http_clone = self.http.clone();
            let announcement_clone = announcement.clone();
            tokio::spawn(async move {
                if let Err(e) = channel_id.say(&http_clone, &announcement_clone).await {
                    tracing::warn!("Failed to send DJ hex message announcement: {}", e);
                }
            });
        }

        Ok(DJState::PlayingHexMessage {
            message: hex_entry.text.clone(),
            started_at: std::time::Instant::now(),
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
        _track_manager: &mut TrackManager,
    ) -> Result<DJState, Box<dyn std::error::Error + Send + Sync>> {
        let noise_entry = self
            .scheduler
            .get_noise_period(idx)
            .ok_or("Noise period index out of bounds")?;

        let mut rng = rand::rng();
        let duration_secs = rng.random_range(
            noise_entry.duration_range_seconds.0..noise_entry.duration_range_seconds.1,
        );
        let duration = Duration::from_secs_f32(duration_secs);

        let noise_type_str = match noise_entry.noise_type {
            NoiseTypeConfig::Static => "static",
            NoiseTypeConfig::Brown => "brown",
        };

        let _track_name = format!("dj_noise_{}", noise_type_str);

        tracing::info!(
            "DJ playing {} noise for {:.1}s in guild {}",
            noise_type_str,
            duration_secs,
            self.guild_id
        );

        Ok(DJState::PlayingNoise {
            noise_type: noise_type_str.to_string(),
            started_at: std::time::Instant::now(),
            duration,
        })
    }

    async fn start_profile_state(
        &mut self,
        idx: usize,
        bot_state: &Data,
    ) -> Result<DJState, Box<dyn std::error::Error + Send + Sync>> {
        let profile_entry = self
            .scheduler
            .get_profile(idx)
            .ok_or("Profile index out of bounds")?;

        let processors = bot_state.audio_processors.read().await;
        let processor_arc = processors.get(&self.guild_id).cloned();
        drop(processors);

        if let Some(processor_arc) = processor_arc
            && let Some(new_profile) = bot_state
                .profile_manager
                .get_profile(&profile_entry.profile_name)
        {
            let mut processor = processor_arc.write().await;
            let fade_ms = profile_entry.fade_duration_seconds * 1000.0;
            processor.start_profile_transition(new_profile.clone(), fade_ms);

            tracing::info!(
                "DJ transitioning to profile '{}' over {:.1}s in guild {}",
                profile_entry.profile_name,
                profile_entry.fade_duration_seconds,
                self.guild_id
            );
        }

        Ok(DJState::TransitioningProfile {
            started_at: std::time::Instant::now(),
            duration: Duration::from_secs_f32(profile_entry.fade_duration_seconds),
        })
    }
}
