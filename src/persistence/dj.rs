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

use crate::persistence::{DjSettings, StateStore};
use serenity::model::id::GuildId;

/// Read one guild's DJ settings, apply `f`, and persist the result.
pub async fn update_dj_settings<F>(
    store: &dyn StateStore,
    guild_id: GuildId,
    f: F,
) -> super::Result<()>
where
    F: FnOnce(&mut DjSettings) -> super::Result<()>,
{
    let mut settings = store.load_dj_settings(guild_id).await?;
    f(&mut settings)?;
    store.save_dj_settings(guild_id, &settings).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::dj::config::HexMessageEntry;
    use crate::persistence::FileStore;

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
        let store = FileStore::new(dir.path().to_path_buf());
        let guild_id = GuildId::new(1);

        update_dj_settings(&store, guild_id, |settings| {
            settings.tracks = Some("tracks.json".to_string());
            Ok(())
        })
        .await
        .unwrap();

        update_dj_settings(&store, guild_id, |settings| {
            settings.hex_message_overrides.set(None, hex_message("one"))
        })
        .await
        .unwrap();

        let settings = store.load_dj_settings(guild_id).await.unwrap();
        assert_eq!(settings.tracks.as_deref(), Some("tracks.json"));
        assert_eq!(settings.hex_message_overrides.items[0].text, "one");
    }

    #[tokio::test]
    async fn a_rejected_mutation_is_not_persisted() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileStore::new(dir.path().to_path_buf());
        let guild_id = GuildId::new(1);

        let result = update_dj_settings(&store, guild_id, |settings| {
            settings.hex_message_overrides.enabled = true;
            settings
                .hex_message_overrides
                .set(Some(3), hex_message("one"))
        })
        .await;

        assert!(result.is_err());
        assert!(
            !store
                .load_dj_settings(guild_id)
                .await
                .unwrap()
                .hex_message_overrides
                .enabled
        );
    }
}
