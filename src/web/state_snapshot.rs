use crate::state::Data;
use serde::{Deserialize, Serialize};
use std::time::SystemTime;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackInfo {
    pub name: String,
    pub filename: String,
    pub volume: f32,
    pub loops: bool,
    pub start_time: SystemTime,
    pub duration_secs: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DJStateInfo {
    pub state_type: String,
    pub details: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioProfileInfo {
    pub name: String,
    pub source: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuildState {
    pub guild_id: String,
    pub in_voice: bool,
    pub tracks: Vec<TrackInfo>,
    pub dj_state: Option<DJStateInfo>,
    pub audio_profile: Option<AudioProfileInfo>,
    pub channel_status_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotSnapshot {
    pub guilds: Vec<GuildState>,
    pub version: String,
    pub commit_hash: String,
    pub current_activity: Option<CurrentActivity>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurrentActivity {
    pub activity_type: String,
    pub status: String,
}

impl BotSnapshot {
    pub async fn capture(bot_state: &Data) -> Self {
        let mut guilds = Vec::new();

        let mut active_guilds: Vec<_> = bot_state
            .guilds
            .lock()
            .unwrap()
            .iter()
            .map(|(id, radio)| (*id, radio.clone()))
            .collect();
        active_guilds.sort_by_key(|(id, _)| *id);

        for (guild_id, guild) in active_guilds {
            // Check if guild is actually in voice
            let in_voice = bot_state.get_voice_channel_id(guild_id).await.is_some();

            let track_snapshots = guild.track_manager.get_all_tracks().await;
            let tracks = track_snapshots
                .iter()
                .map(|t| TrackInfo {
                    name: t.name.clone(),
                    filename: t.filename.clone(),
                    volume: t.volume,
                    loops: t.loops,
                    start_time: t.start_time,
                    duration_secs: t.duration.map(|d| d.as_secs_f64()),
                })
                .collect();

            let dj_state = if let Some(state_arc) = bot_state.dj_state(guild_id).await {
                let state = state_arc.read().await;
                Some(dj_state_to_info(&state))
            } else {
                None
            };

            // Get audio profile info
            let processor = guild.audio_processor.read().await;
            let profile_name = processor.current_profile_name().to_string();

            let source = if let Some(state_arc) = bot_state.dj_state(guild_id).await {
                let state = state_arc.read().await;
                Some(state.profile_source().to_string())
            } else {
                None
            };

            let audio_profile = Some(AudioProfileInfo {
                name: profile_name,
                source,
            });

            guilds.push(GuildState {
                guild_id: guild_id.to_string(),
                in_voice,
                tracks,
                dj_state,
                audio_profile,
                channel_status_enabled: !bot_state
                    .dj_settings
                    .get(guild_id)
                    .await
                    .channel_status_disabled,
            });
        }

        // Get current bot activity
        let current_activity = bot_state.activity_manager.current().await.map(|entry| {
            let activity_type = match entry.activity_type {
                crate::activity::ActivityType::Listening => "listening",
                crate::activity::ActivityType::Playing => "playing",
                crate::activity::ActivityType::Streaming => "streaming",
                crate::activity::ActivityType::Custom => "custom",
            };
            CurrentActivity {
                activity_type: activity_type.to_string(),
                status: entry.status,
            }
        });

        BotSnapshot {
            guilds,
            version: env!("BUILD_RUN_NUMBER").to_string(),
            commit_hash: env!("BUILD_COMMIT_HASH").to_string(),
            current_activity,
        }
    }
}

fn dj_state_to_info(state: &crate::audio::dj::state_machine::DJState) -> DJStateInfo {
    let display = state.segment.state_display();
    let elapsed = state.segment.elapsed().as_secs_f32();
    let time = match display.total {
        Some(total) => format!("{elapsed:.1}s / {:.1}s", total.as_secs_f32()),
        None => format!("{elapsed:.1}s"),
    };
    let details = match display.subject {
        Some(subject) => format!("{subject} ({time})"),
        None => time,
    };
    DJStateInfo {
        state_type: display.name.to_string(),
        details,
    }
}
