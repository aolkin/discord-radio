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
                forced_profile: _, // Don't persist forced_profile, it's transient
            } => DJStateMachineState::PlayingTrack {
                track_name: track_name.clone(),
                filename: filename.clone(),
                started_at: instant_to_systime(started_at),
                duration_secs: duration.as_secs_f32(),
            },
            DJState::PlayingHexMessage {
                message,
                started_at,
                target_loops,
                forced_profile: _, // Don't persist forced_profile, it's transient
                status_message: _, // Don't persist status_message, it will be regenerated
            } => DJStateMachineState::PlayingHexMessage {
                message: message.clone(),
                started_at: instant_to_systime(started_at),
                target_loops: *target_loops,
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
            } => Ok(DJState::PlayingTrack {
                track_name: track_name.clone(),
                filename: filename.clone(),
                started_at: systime_to_instant(started_at)?,
                duration: Duration::from_secs_f32(*duration_secs),
                forced_profile: None, // Restored state doesn't have forced profile
            }),
            DJStateMachineState::PlayingHexMessage {
                message,
                started_at,
                target_loops,
            } => Ok(DJState::PlayingHexMessage {
                message: message.clone(),
                started_at: systime_to_instant(started_at)?,
                target_loops: *target_loops,
                forced_profile: None, // Restored state doesn't have forced profile
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
