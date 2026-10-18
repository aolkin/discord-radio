use serde::{Deserialize, Serialize};
use serenity::model::id::{ChannelId, GuildId};
use std::time::SystemTime;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TrackState {
    pub name: String,
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

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct MultiTrackPlaybackState {
    pub tracks: Vec<TrackState>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct DjSettings {
    #[serde(default)]
    pub channel_status_disabled: bool,
}

#[derive(Deserialize, Debug)]
pub struct DJState {
    pub config_name: Option<String>,
    #[serde(default)]
    pub announcement_channel_id: Option<u64>,
    #[serde(default)]
    pub state_machine: Option<crate::audio::dj::state_machine::DJState>,
}

#[derive(Serialize, Debug)]
pub struct DJStateSnapshot<'a> {
    pub config_name: Option<&'a str>,
    pub announcement_channel_id: Option<u64>,
    pub state_machine: &'a crate::audio::dj::state_machine::DJState,
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
