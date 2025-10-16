use crate::audio::dj::state_machine::DJState;
use crate::persistence::types::DJStateMachineState;
use std::time::{Duration, Instant, SystemTime};

impl From<&DJState> for DJStateMachineState {
    fn from(state: &DJState) -> Self {
        let instant_to_systime = |instant: &Instant| -> SystemTime {
            let elapsed = instant.elapsed();
            SystemTime::now()
                .checked_sub(elapsed)
                .unwrap_or_else(SystemTime::now)
        };

        match state {
            DJState::PlayingTrack {
                track_name,
                filename,
                started_at,
                duration,
                forced_profile,
                status_message,
            } => DJStateMachineState::PlayingTrack {
                track_name: track_name.clone(),
                filename: filename.clone(),
                started_at: instant_to_systime(started_at),
                duration_secs: duration.as_secs_f32(),
                forced_profile: forced_profile.clone(),
                status_message: status_message.clone(),
            },
            DJState::PlayingHexMessage {
                message,
                started_at,
                target_loops,
                forced_profile,
                status_message: _, // Don't persist status_message, it will be regenerated
            } => DJStateMachineState::PlayingHexMessage {
                message: message.clone(),
                started_at: instant_to_systime(started_at),
                target_loops: *target_loops,
                forced_profile: forced_profile.clone(),
            },
            DJState::PlayingNoise {
                noise_profile,
                started_at,
                duration,
            } => DJStateMachineState::PlayingNoise {
                noise_profile: noise_profile.clone(),
                started_at: instant_to_systime(started_at),
                duration_secs: duration.as_secs_f32(),
            },
            DJState::Idle {
                started_at,
                duration,
            } => DJStateMachineState::Idle {
                started_at: instant_to_systime(started_at),
                duration_secs: duration.as_secs_f32(),
            },
            DJState::Stopped => DJStateMachineState::Stopped,
        }
    }
}

impl TryFrom<&DJStateMachineState> for DJState {
    type Error = ();

    fn try_from(persist_state: &DJStateMachineState) -> Result<Self, Self::Error> {
        let systime_to_instant = |systime: &SystemTime| -> Result<Instant, ()> {
            let elapsed = systime.elapsed().map_err(|_| ())?;
            Instant::now().checked_sub(elapsed).ok_or(())
        };

        match persist_state {
            DJStateMachineState::PlayingTrack {
                track_name,
                filename,
                started_at,
                duration_secs,
                forced_profile,
                status_message,
            } => Ok(DJState::PlayingTrack {
                track_name: track_name.clone(),
                filename: filename.clone(),
                started_at: systime_to_instant(started_at)?,
                duration: Duration::from_secs_f32(*duration_secs),
                forced_profile: forced_profile.clone(),
                status_message: status_message.clone(),
            }),
            DJStateMachineState::PlayingHexMessage {
                message,
                started_at,
                target_loops,
                forced_profile,
            } => Ok(DJState::PlayingHexMessage {
                message: message.clone(),
                started_at: systime_to_instant(started_at)?,
                target_loops: *target_loops,
                forced_profile: forced_profile.clone(),
                status_message: None, // Restored state doesn't have status message, will be regenerated
            }),
            DJStateMachineState::PlayingNoise {
                noise_profile,
                started_at,
                duration_secs,
            } => Ok(DJState::PlayingNoise {
                noise_profile: noise_profile.clone(),
                started_at: systime_to_instant(started_at)?,
                duration: Duration::from_secs_f32(*duration_secs),
            }),
            DJStateMachineState::Idle {
                started_at,
                duration_secs,
            } => Ok(DJState::Idle {
                started_at: systime_to_instant(started_at)?,
                duration: Duration::from_secs_f32(*duration_secs),
            }),
            DJStateMachineState::Stopped => Ok(DJState::Stopped),
            _ => Ok(DJState::Stopped), // Handle any other states (e.g., old TransitioningProfile)
        }
    }
}

// DJ Config Overrides
use crate::persistence::types::DJConfigOverrides;
use crate::persistence::utils::save_json_to_file;
use std::sync::Arc;
use tokio::sync::RwLock;

impl DJConfigOverrides {
    pub fn load_from_file(
        path: &std::path::Path,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let contents = std::fs::read_to_string(path)?;
        let overrides: DJConfigOverrides = serde_json::from_str(&contents)?;
        Ok(overrides)
    }
}

/// A wrapper around DJConfigOverrides that auto-saves on mutations
pub struct DJConfigOverridesStore {
    overrides: Arc<RwLock<DJConfigOverrides>>,
    path: std::path::PathBuf,
}

impl DJConfigOverridesStore {
    pub fn new(overrides: DJConfigOverrides, path: std::path::PathBuf) -> Self {
        Self {
            overrides: Arc::new(RwLock::new(overrides)),
            path,
        }
    }

    pub fn get_arc(&self) -> Arc<RwLock<DJConfigOverrides>> {
        self.overrides.clone()
    }

    async fn save(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let overrides = self.overrides.read().await;
        save_json_to_file(&*overrides, &self.path).await
    }

    pub async fn set_hex_message(
        &self,
        index: Option<usize>,
        entry: crate::audio::dj::config::HexMessageEntry,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        {
            let mut overrides = self.overrides.write().await;
            if let Some(idx) = index {
                if idx < overrides.hex_messages.items.len() {
                    overrides.hex_messages.items[idx] = entry;
                } else {
                    return Err("Index out of bounds".into());
                }
            } else {
                overrides.hex_messages.items.push(entry);
            }
        }
        self.save().await
    }

    pub async fn delete_hex_message(
        &self,
        index: usize,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        {
            let mut overrides = self.overrides.write().await;
            if index >= overrides.hex_messages.items.len() {
                return Err("Index out of bounds".into());
            }
            overrides.hex_messages.items.remove(index);
        }
        self.save().await
    }

    pub async fn set_announcement(
        &self,
        index: Option<usize>,
        text: String,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        {
            let mut overrides = self.overrides.write().await;
            if let Some(idx) = index {
                if idx < overrides.hex_message_announcements.items.len() {
                    overrides.hex_message_announcements.items[idx] = text;
                } else {
                    return Err("Index out of bounds".into());
                }
            } else {
                overrides.hex_message_announcements.items.push(text);
            }
        }
        self.save().await
    }

    pub async fn delete_announcement(
        &self,
        index: usize,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        {
            let mut overrides = self.overrides.write().await;
            if index >= overrides.hex_message_announcements.items.len() {
                return Err("Index out of bounds".into());
            }
            overrides.hex_message_announcements.items.remove(index);
        }
        self.save().await
    }

    pub async fn toggle_category(
        &self,
        category: &str,
        enabled: bool,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        {
            let mut overrides = self.overrides.write().await;
            match category {
                "hex_messages" => {
                    overrides.hex_messages.enabled = enabled;
                }
                "hex_message_announcements" => {
                    overrides.hex_message_announcements.enabled = enabled;
                }
                _ => return Err("Unknown category".into()),
            }
        }
        self.save().await
    }
}
