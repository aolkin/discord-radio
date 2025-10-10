use serde::{Deserialize, Serialize};
use std::time::SystemTime;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct MessagePlaybackState {
    pub message: String,
    pub current_position: usize,
}

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
