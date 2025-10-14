use crate::state::Data;
use serde::{Deserialize, Serialize};
use serenity::model::id::GuildId;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackInfo {
    pub name: String,
    pub filename: String,
    pub volume: f32,
    pub loops: bool,
    pub start_time: std::time::SystemTime,
    pub duration_secs: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HexPlaybackInfo {
    pub message: Option<String>,
    pub current_position: usize,
    pub volume: f32,
    pub current_loop: usize,
    pub target_loops: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DJStateInfo {
    pub state_type: String,
    pub details: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioProfileInfo {
    pub name: String,
    pub forced: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuildState {
    pub guild_id: String,
    pub in_voice: bool,
    pub tracks: Vec<TrackInfo>,
    pub hex_playback: Option<HexPlaybackInfo>,
    pub dj_state: Option<DJStateInfo>,
    pub audio_profile: Option<AudioProfileInfo>,
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

        // Collect all guild IDs from various state maps
        let mut guild_ids_set = std::collections::HashSet::new();

        // Get guild IDs from voice connections
        let voice_connections = bot_state.voice_connections.read().await;
        guild_ids_set.extend(voice_connections.keys().copied());
        drop(voice_connections);

        // Get guild IDs from other state maps
        let track_managers = bot_state.track_managers.read().await;
        guild_ids_set.extend(track_managers.keys().copied());
        drop(track_managers);

        let hex_states = bot_state.hex_playback_states.read().await;
        guild_ids_set.extend(hex_states.keys().copied());
        drop(hex_states);

        let audio_processors = bot_state.audio_processors.read().await;
        guild_ids_set.extend(audio_processors.keys().copied());
        drop(audio_processors);

        let dj_states = bot_state.dj_states.read().await;
        guild_ids_set.extend(dj_states.keys().copied());
        drop(dj_states);

        let guild_ids: Vec<GuildId> = guild_ids_set.into_iter().collect();

        for guild_id in guild_ids {
            // Check if guild is actually in voice
            let voice_connections = bot_state.voice_connections.read().await;
            let in_voice = voice_connections.contains_key(&guild_id);
            drop(voice_connections);

            let mut guild_state = GuildState {
                guild_id: guild_id.to_string(),
                in_voice,
                tracks: Vec::new(),
                hex_playback: None,
                dj_state: None,
                audio_profile: None,
            };

            // Get track info
            let track_managers = bot_state.track_managers.read().await;
            if let Some(manager_arc) = track_managers.get(&guild_id) {
                let manager = manager_arc.lock().await;
                let track_snapshots = manager.get_all_tracks().await;
                guild_state.tracks = track_snapshots
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
            }
            drop(track_managers);

            // Get hex playback state
            let hex_states = bot_state.hex_playback_states.read().await;
            if let Some(state_arc) = hex_states.get(&guild_id) {
                let state = state_arc.read().await;
                guild_state.hex_playback = Some(HexPlaybackInfo {
                    message: state.message.clone(),
                    current_position: state.current_position,
                    volume: state.volume,
                    current_loop: state.current_loop,
                    target_loops: state.target_loops,
                });
            }
            drop(hex_states);

            // Get DJ state
            let dj_states = bot_state.dj_states.read().await;
            if let Some(state_arc) = dj_states.get(&guild_id) {
                let state = state_arc.read().await;
                guild_state.dj_state = Some(dj_state_to_info(&state));
            }
            drop(dj_states);

            // Get audio profile info
            let audio_processors = bot_state.audio_processors.read().await;
            if let Some(processor_arc) = audio_processors.get(&guild_id) {
                let processor = processor_arc.read().await;
                let profile_name = processor.current_profile_name().to_string();

                // Check if profile is forced by DJ state
                let dj_states = bot_state.dj_states.read().await;
                let is_forced = if let Some(state_arc) = dj_states.get(&guild_id) {
                    let state = state_arc.read().await;
                    state.forced_profile().is_some()
                } else {
                    false
                };
                drop(dj_states);

                guild_state.audio_profile = Some(AudioProfileInfo {
                    name: profile_name,
                    forced: is_forced,
                });
            }
            drop(audio_processors);

            guilds.push(guild_state);
        }

        // Get current bot activity
        let current_activity = bot_state.activity_manager.current().await.map(|entry| {
            let activity_type = match entry.activity_type {
                crate::voice_status::ActivityType::Listening => "listening",
                crate::voice_status::ActivityType::Playing => "playing",
                crate::voice_status::ActivityType::Streaming => "streaming",
                crate::voice_status::ActivityType::Custom => "custom",
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
    use crate::audio::dj::state_machine::DJState;

    match state {
        DJState::PlayingTrack {
            track_name,
            duration,
            started_at,
            ..
        } => DJStateInfo {
            state_type: "PlayingTrack".to_string(),
            details: format!(
                "{} ({:.1}s / {:.1}s)",
                track_name,
                started_at.elapsed().as_secs_f32(),
                duration.as_secs_f32()
            ),
        },
        DJState::PlayingHexMessage {
            message,
            target_loops,
            ..
        } => DJStateInfo {
            state_type: "PlayingHexMessage".to_string(),
            details: format!("Message: {} (loops: {})", message, target_loops),
        },
        DJState::PlayingNoise {
            noise_profile,
            duration,
            started_at,
        } => DJStateInfo {
            state_type: "PlayingNoise".to_string(),
            details: format!(
                "Profile: {} ({:.1}s / {:.1}s)",
                noise_profile,
                started_at.elapsed().as_secs_f32(),
                duration.as_secs_f32()
            ),
        },
        DJState::Idle {
            duration,
            started_at,
        } => DJStateInfo {
            state_type: "Idle".to_string(),
            details: format!(
                "{:.1}s / {:.1}s",
                started_at.elapsed().as_secs_f32(),
                duration.as_secs_f32()
            ),
        },
        DJState::Stopped => DJStateInfo {
            state_type: "Stopped".to_string(),
            details: "Stopped".to_string(),
        },
    }
}
