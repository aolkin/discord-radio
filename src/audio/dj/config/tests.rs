use super::*;
use crate::bucket::{FileResolver, ObjectStore};
use crate::persistence::DjSettings;
use std::sync::Arc;

fn local_file_config() -> DJConfig {
    DJConfig {
        name: "test".to_string(),
        track_pool: Vec::new(),
        hex_messages: vec![HexMessageEntry {
            text: "from the local file".to_string(),
            weight: 1,
            signal_profile: None,
            loop_min: None,
            loop_max: None,
            announcement: None,
        }],
        hex_message_announcements: Some(vec!["from the local file".to_string()]),
        hex_message_defaults: HexMessageDefaults {
            loop_min: 9,
            loop_max: 9,
            signal_profile: Some("local".to_string()),
        },
        noise_periods: vec![NoisePeriodEntry {
            noise_profile: "local".to_string(),
            min_duration_seconds: 1.0,
            max_duration_seconds: 2.0,
            weight: 1,
        }],
        signal_profiles: vec![SignalProfileEntry {
            profile_name: "local".to_string(),
            weight: 1,
            fade_duration_seconds: 1.0,
            min_time_seconds: 1.0,
            max_time_seconds: 2.0,
        }],
        state_weights: StateWeights {
            track: 7,
            hex_message: 7,
            noise: 7,
        },
        recent_history_size: 12,
        duplicate_penalty_multiplier: 0.5,
        channel_status: Some("from the local file".to_string()),
    }
}

async fn resolver(content_path: &std::path::Path) -> FileResolver {
    let file_cache = Arc::new(
        ObjectStore::new(content_path.to_path_buf(), None, None)
            .await
            .unwrap(),
    );
    FileResolver::new(content_path.to_string_lossy().to_string(), file_cache)
}

#[tokio::test]
async fn slots_replace_the_local_file_values_whether_set_or_cleared() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("hex.json"),
        r#"{"defaults":{"loop_min":4,"loop_max":6},"announcements":["slot"],
                "messages":[{"text":"slot","weight":2,"signal_profile":null,
                "loop_min":null,"loop_max":null,"announcement":null}]}"#,
    )
    .unwrap();
    std::fs::write(
        dir.path().join("config.json"),
        r#"{"state_weights":{"track":3,"hex_message":0,"noise":0},
                "playback":{"recent_history_size":2,"duplicate_penalty_multiplier":0.25},
                "signal_profiles":[{"profile_name":"slot","weight":2,
                "fade_duration_seconds":3.0,"min_time_seconds":4.0,"max_time_seconds":5.0}],
                "channel_status":"from the slot"}"#,
    )
    .unwrap();
    let resolver = resolver(dir.path()).await;

    let mut config = local_file_config();
    config
        .apply_dj_settings(
            &DjSettings {
                hex_messages: Some("hex.json".to_string()),
                config: Some("config.json".to_string()),
                ..Default::default()
            },
            &resolver,
        )
        .await
        .unwrap();

    assert_eq!(config.hex_messages.len(), 1);
    assert_eq!(config.hex_messages[0].text, "slot");
    assert_eq!(
        config.hex_message_announcements,
        Some(vec!["slot".to_string()])
    );
    assert_eq!(config.hex_message_defaults.loop_max, 6);
    assert!(config.noise_periods.is_empty());
    assert_eq!(config.state_weights.track, 3);
    assert_eq!(config.recent_history_size, 2);
    assert_eq!(config.duplicate_penalty_multiplier, 0.25);
    assert_eq!(config.signal_profiles.len(), 1);
    assert_eq!(config.signal_profiles[0].profile_name, "slot");
    assert_eq!(config.channel_status, Some("from the slot".to_string()));

    let mut config = local_file_config();
    config
        .apply_dj_settings(&DjSettings::default(), &resolver)
        .await
        .unwrap();

    assert!(config.hex_messages.is_empty());
    assert_eq!(config.hex_message_announcements, Some(Vec::new()));
    assert_eq!(config.hex_message_defaults.loop_max, default_loop_max());
    assert!(config.noise_periods.is_empty());
    assert_eq!(config.state_weights.track, StateWeights::default().track);
    assert_eq!(
        config.recent_history_size,
        PlaybackSettings::default().recent_history_size
    );
    assert!(config.signal_profiles.is_empty());
    assert_eq!(config.channel_status, None);
}
