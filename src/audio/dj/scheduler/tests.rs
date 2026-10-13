use super::*;
use crate::audio::dj::config::StateWeights;

fn config(track_pool: Vec<TrackEntry>) -> DJConfig {
    DJConfig {
        name: "test".to_string(),
        track_pool,
        hex_messages: Vec::new(),
        hex_message_announcements: None,
        hex_message_defaults: Default::default(),
        noise_periods: Vec::new(),
        signal_profiles: Vec::new(),
        state_weights: StateWeights {
            track: 1,
            hex_message: 1,
            noise: 1,
        },
        recent_history_size: 4,
        duplicate_penalty_multiplier: 0.5,
        channel_status: None,
    }
}

fn track() -> TrackEntry {
    TrackEntry {
        filename: "audio/song.ogg".to_string(),
        weight: 1,
        max_duration_seconds: None,
        allow_subsection: None,
        signal_profile: None,
        volume: None,
        channel_status: None,
    }
}

#[test]
fn all_pools_empty_falls_back_to_noise() {
    let scheduler = WeightedScheduler::new(config(Vec::new()));
    for _ in 0..100 {
        assert!(matches!(scheduler.choose_state_type(), DJStateType::Noise));
    }
}

#[test]
fn empty_categories_are_skipped() {
    let scheduler = WeightedScheduler::new(config(vec![track()]));
    for _ in 0..100 {
        assert!(matches!(scheduler.choose_state_type(), DJStateType::Track,));
    }
}
