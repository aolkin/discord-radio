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

use super::utils::save_json_to_file;
use crate::persistence::DjSettings;
use serenity::model::id::GuildId;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct DjSettingsStore {
    settings: Arc<RwLock<HashMap<GuildId, DjSettings>>>,
    path: PathBuf,
}

impl DjSettingsStore {
    pub fn new(path: PathBuf) -> Self {
        let settings = std::fs::read_to_string(&path)
            .ok()
            .and_then(|contents| serde_json::from_str(&contents).ok())
            .unwrap_or_default();
        Self {
            settings: Arc::new(RwLock::new(settings)),
            path,
        }
    }

    pub async fn get(&self, guild_id: GuildId) -> DjSettings {
        self.settings
            .read()
            .await
            .get(&guild_id)
            .cloned()
            .unwrap_or_default()
    }

    pub async fn update<F>(&self, guild_id: GuildId, f: F) -> super::Result<()>
    where
        F: FnOnce(&mut DjSettings) -> super::Result<()>,
    {
        let mut map = self.settings.write().await;
        let mut entry = map.get(&guild_id).cloned().unwrap_or_default();
        f(&mut entry)?;
        map.insert(guild_id, entry);
        save_json_to_file(&*map, &self.path).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::dj::config::HexMessageEntry;

    fn hex_message(text: &str) -> HexMessageEntry {
        HexMessageEntry {
            text: text.to_string(),
            weight: 1,
            signal_profile: None,
            loop_min: None,
            loop_max: None,
            announcement: None,
        }
    }

    #[tokio::test]
    async fn overrides_persist_alongside_content_slots() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("dj_settings.json");
        let store = DjSettingsStore::new(path.clone());
        let guild_id = GuildId::new(1);

        store
            .update(guild_id, |settings| {
                settings.tracks = Some("tracks.json".to_string());
                Ok(())
            })
            .await
            .unwrap();

        store
            .update(guild_id, |settings| {
                settings.hex_message_overrides.set(None, hex_message("one"))
            })
            .await
            .unwrap();

        let settings = DjSettingsStore::new(path).get(guild_id).await;
        assert_eq!(settings.tracks.as_deref(), Some("tracks.json"));
        assert_eq!(settings.hex_message_overrides.items[0].text, "one");
    }

    #[tokio::test]
    async fn a_rejected_mutation_is_not_persisted() {
        let dir = tempfile::tempdir().unwrap();
        let store = DjSettingsStore::new(dir.path().join("dj_settings.json"));
        let guild_id = GuildId::new(1);

        let result = store
            .update(guild_id, |settings| {
                settings.hex_message_overrides.enabled = true;
                settings
                    .hex_message_overrides
                    .set(Some(3), hex_message("one"))
            })
            .await;

        assert!(result.is_err());
        assert!(!store.get(guild_id).await.hex_message_overrides.enabled);
    }
}
