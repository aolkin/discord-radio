use crate::audio::dj::config::{DJConfig, NoiseTypeConfig};
use crate::audio::dj::scheduler::{DJStateType, WeightedScheduler};
use crate::audio::tracks::{StartTrackArgs, TrackManager};
use crate::state::Data;
use rand::Rng;
use serenity::model::id::GuildId;
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

    /// Convert to a persistable state
    pub fn to_persistable(&self) -> crate::persistence::DJStateMachineState {
        use crate::persistence::DJStateMachineState as PersistState;
        use std::time::SystemTime;

        // Helper to convert Instant to SystemTime
        let instant_to_systime = |instant: &std::time::Instant| -> SystemTime {
            let elapsed = instant.elapsed();
            SystemTime::now()
                .checked_sub(elapsed)
                .unwrap_or_else(SystemTime::now)
        };

        match self {
            DJState::PlayingTrack {
                track_name,
                filename,
                started_at,
                duration,
            } => PersistState::PlayingTrack {
                track_name: track_name.clone(),
                filename: filename.clone(),
                started_at: instant_to_systime(started_at),
                duration_secs: duration.as_secs_f32(),
            },
            DJState::PlayingHexMessage {
                message,
                started_at,
            } => PersistState::PlayingHexMessage {
                message: message.clone(),
                started_at: instant_to_systime(started_at),
            },
            DJState::PlayingNoise {
                noise_type,
                started_at,
                duration,
            } => PersistState::PlayingNoise {
                noise_type: noise_type.clone(),
                started_at: instant_to_systime(started_at),
                duration_secs: duration.as_secs_f32(),
            },
            DJState::TransitioningProfile {
                started_at,
                duration,
            } => PersistState::TransitioningProfile {
                started_at: instant_to_systime(started_at),
                duration_secs: duration.as_secs_f32(),
            },
            DJState::Idle {
                started_at,
                duration,
            } => PersistState::Idle {
                started_at: instant_to_systime(started_at),
                duration_secs: duration.as_secs_f32(),
            },
            DJState::Stopped => PersistState::Stopped,
        }
    }

    /// Create from a persistable state
    pub fn from_persistable(
        persist_state: &crate::persistence::DJStateMachineState,
    ) -> Option<Self> {
        use crate::persistence::DJStateMachineState as PersistState;
        use std::time::{Duration, Instant};

        // Helper to convert SystemTime to Instant
        let systime_to_instant = |systime: &std::time::SystemTime| -> Option<Instant> {
            let elapsed = systime.elapsed().ok()?;
            Instant::now().checked_sub(elapsed)
        };

        match persist_state {
            PersistState::PlayingTrack {
                track_name,
                filename,
                started_at,
                duration_secs,
            } => Some(DJState::PlayingTrack {
                track_name: track_name.clone(),
                filename: filename.clone(),
                started_at: systime_to_instant(started_at)?,
                duration: Duration::from_secs_f32(*duration_secs),
            }),
            PersistState::PlayingHexMessage {
                message,
                started_at,
            } => Some(DJState::PlayingHexMessage {
                message: message.clone(),
                started_at: systime_to_instant(started_at)?,
            }),
            PersistState::PlayingNoise {
                noise_type,
                started_at,
                duration_secs,
            } => Some(DJState::PlayingNoise {
                noise_type: noise_type.clone(),
                started_at: systime_to_instant(started_at)?,
                duration: Duration::from_secs_f32(*duration_secs),
            }),
            PersistState::TransitioningProfile {
                started_at,
                duration_secs,
            } => Some(DJState::TransitioningProfile {
                started_at: systime_to_instant(started_at)?,
                duration: Duration::from_secs_f32(*duration_secs),
            }),
            PersistState::Idle {
                started_at,
                duration_secs,
            } => Some(DJState::Idle {
                started_at: systime_to_instant(started_at)?,
                duration: Duration::from_secs_f32(*duration_secs),
            }),
            PersistState::Stopped => Some(DJState::Stopped),
        }
    }
}

pub struct DJStateMachine {
    current_state: DJState,
    scheduler: WeightedScheduler,
    guild_id: GuildId,
    hex_audio_dir: String,
}

impl DJStateMachine {
    pub fn new(
        config: DJConfig,
        guild_id: GuildId,
        hex_audio_dir: String,
        restored_state: Option<crate::persistence::DJStateMachineState>,
    ) -> Self {
        let current_state = if let Some(state) = restored_state {
            DJState::from_persistable(&state).unwrap_or_else(|| {
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

        Self {
            current_state,
            scheduler: WeightedScheduler::new(config),
            guild_id,
            hex_audio_dir,
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

    pub fn force_complete(&mut self) {
        match &mut self.current_state {
            DJState::PlayingTrack {
                started_at,
                duration,
                ..
            } => {
                *started_at = std::time::Instant::now() - *duration;
            }
            DJState::PlayingNoise {
                started_at,
                duration,
                ..
            } => {
                *started_at = std::time::Instant::now() - *duration;
            }
            DJState::TransitioningProfile {
                started_at,
                duration,
                ..
            } => {
                *started_at = std::time::Instant::now() - *duration;
            }
            DJState::Idle {
                started_at,
                duration,
            } => {
                *started_at = std::time::Instant::now() - *duration;
            }
            DJState::PlayingHexMessage { started_at, .. } => {
                *started_at = std::time::Instant::now() - Duration::from_secs(999);
            }
            DJState::Stopped => {}
        }
    }

    pub async fn advance(
        &mut self,
        track_manager: &mut TrackManager,
        bot_state: &Data,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if !self.current_state.is_complete() {
            return Ok(());
        }

        self.cleanup_current_state(track_manager).await?;

        let next_state_type = self.scheduler.next_state();
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

        let full_path = format!("{}/{}", self.hex_audio_dir, track_entry.filename);
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

        {
            let mut state = hex_playback_state.write().await;
            *state = crate::state::HexPlaybackState::playing(hex_entry.text.clone(), 0, 1.0);
        }

        tracing::info!(
            "DJ playing hex message '{}' in guild {}",
            hex_entry.text,
            self.guild_id
        );

        Ok(DJState::PlayingHexMessage {
            message: hex_entry.text.clone(),
            started_at: std::time::Instant::now(),
        })
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
