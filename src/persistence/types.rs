use crate::activity::ActivityType;
use serde::{Deserialize, Serialize};
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

#[derive(Deserialize, Debug)]
pub struct DJState {
    #[serde(default)]
    pub state_machine: Option<crate::audio::dj::state_machine::DJState>,
    pub saved_at: SystemTime,
    pub stopped: bool,
}

#[derive(Serialize, Debug)]
pub struct DJStateSnapshot<'a> {
    pub state_machine: &'a crate::audio::dj::state_machine::DJState,
    pub saved_at: SystemTime,
    pub stopped: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ActivityState {
    pub activity_type: ActivityType,
    pub status: String,
}
