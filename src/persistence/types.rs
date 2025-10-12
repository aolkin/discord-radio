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
    pub state_machine: Option<DJStateMachineState>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum DJStateMachineState {
    PlayingTrack {
        track_name: String,
        filename: String,
        started_at: SystemTime,
        duration_secs: f32,
    },
    PlayingHexMessage {
        message: String,
        started_at: SystemTime,
    },
    PlayingNoise {
        noise_type: String,
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
