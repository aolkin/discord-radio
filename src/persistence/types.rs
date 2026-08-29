use serde::{Deserialize, Serialize};
use serenity::model::id::{ChannelId, GuildId};
use std::time::SystemTime;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct MessagePlaybackState {
    pub message: String,
    pub current_position: usize,
    #[serde(default)]
    pub current_loop: usize,
    #[serde(default)]
    pub target_loops: Option<usize>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TrackState {
    pub name: String,
    /// The filename or `s3://` key as originally requested; pass it through
    /// `resolve_track_path` before use.
    pub filename: String,
    pub volume: f32,
    #[serde(default = "default_loops")]
    pub loops: bool,
    #[serde(default)]
    pub start_time: Option<SystemTime>,
}

fn default_loops() -> bool {
    true
}

fn default_target_loops() -> usize {
    1
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct MultiTrackPlaybackState {
    pub tracks: Vec<TrackState>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ProfileState {
    pub profile_name: String,
    pub bypass: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DJState {
    pub config_name: String,
    pub running: bool,
    #[serde(default)]
    pub announcement_channel_id: Option<u64>,
    #[serde(default)]
    pub state_machine: Option<DJStateMachineState>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum DJStateMachineState {
    PlayingTrack {
        track_name: String,
        filename: String,
        started_at: SystemTime,
        duration_secs: f32,
        #[serde(default)]
        forced_profile: Option<String>,
        #[serde(default)]
        status_message: Option<String>,
    },
    PlayingHexMessage {
        message: String,
        started_at: SystemTime,
        #[serde(default = "default_target_loops")]
        target_loops: usize,
        #[serde(default)]
        forced_profile: Option<String>,
    },
    PlayingNoise {
        noise_profile: String,
        started_at: SystemTime,
        duration_secs: f32,
    },
    TransitioningProfile {
        started_at: SystemTime,
        duration_secs: f32,
    },
    Idle {
        started_at: SystemTime,
        duration_secs: f32,
    },
    Stopped,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct DJConfigOverrides {
    #[serde(default)]
    pub hex_messages: DJConfigOverrideCategory<crate::audio::dj::config::HexMessageEntry>,
    #[serde(default)]
    pub hex_message_announcements: DJConfigOverrideCategory<String>,
    #[serde(default)]
    pub state_weights: DJConfigOverrideSingle<crate::audio::dj::config::StateWeights>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DJConfigOverrideCategory<T> {
    pub enabled: bool,
    pub items: Vec<T>,
}

impl<T> Default for DJConfigOverrideCategory<T> {
    fn default() -> Self {
        Self {
            enabled: false,
            items: Vec::new(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DJConfigOverrideSingle<T> {
    pub enabled: bool,
    pub value: Option<T>,
}

impl<T> Default for DJConfigOverrideSingle<T> {
    fn default() -> Self {
        Self {
            enabled: false,
            value: None,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct RegisteredChannel {
    pub channel_id: ChannelId,
    pub guild_id: GuildId,
    pub name: String,
    pub channel_type: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ActivityState {
    pub activity_type: String,
    pub status: String,
}
