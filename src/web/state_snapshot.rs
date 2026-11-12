use crate::activity::ActivityEntry;
use crate::audio::dj::state_machine::ChannelStatus;
use crate::audio::tracks::TrackSnapshot;
use crate::state::Data;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DJStateInfo {
    pub state_type: String,
    pub details: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioProfileInfo {
    pub name: String,
    pub expected_name: String,
    pub transitioning: bool,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuildState {
    pub guild_id: String,
    pub in_voice: bool,
    pub tracks: Vec<TrackSnapshot>,
    pub dj_state: DJStateInfo,
    pub dj_config: Option<String>,
    pub audio_profile: AudioProfileInfo,
    pub channel_status: ChannelStatusInfo,
    pub announcement_channels: Vec<AnnouncementChannelInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnnouncementChannelInfo {
    pub channel_id: String,
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelStatusInfo {
    pub status: Option<String>,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotSnapshot {
    pub guilds: Vec<GuildState>,
    pub version: String,
    pub commit_hash: String,
    pub current_activity: Option<ActivityEntry>,
}

impl BotSnapshot {
    pub async fn capture(bot_state: &Data) -> Self {
        let mut guilds = Vec::new();

        for (guild_id, guild) in bot_state.guild_radios() {
            // Check if guild is actually in voice
            let in_voice = bot_state.get_voice_channel_id(guild_id).await.is_some();

            let tracks = guild.track_manager.get_all_tracks().await;

            let (dj_state, profile, channel_status, announcement_channels) = {
                let dj_state_lock = guild.dj_manager.state();
                let state = dj_state_lock.read().await;
                (
                    dj_state_to_info(&state),
                    state.signal_profile(),
                    state.channel_status(),
                    state
                        .announcement_channels
                        .iter()
                        .map(|channel| AnnouncementChannelInfo {
                            channel_id: channel.to_string(),
                            name: bot_state.cache.get().and_then(|cache| {
                                cache
                                    .guild(guild_id)
                                    .and_then(|g| g.channels.get(channel).map(|c| c.name.clone()))
                            }),
                        })
                        .collect(),
                )
            };

            // Get audio profile info
            let processor = guild.audio_processor.read().await;
            let profile_name = processor.current_profile_name().to_string();

            let audio_profile = AudioProfileInfo {
                name: profile_name,
                expected_name: profile.value.name,
                transitioning: processor.is_transitioning(),
                source: profile.tier.to_string(),
            };

            guilds.push(GuildState {
                guild_id: guild_id.to_string(),
                in_voice,
                tracks,
                dj_state,
                dj_config: guild.dj_manager.config_name(),
                audio_profile,
                channel_status: ChannelStatusInfo {
                    status: match channel_status.value {
                        ChannelStatus::Unmanaged => None,
                        ChannelStatus::Fixed(status) => Some(status),
                    },
                    source: channel_status.tier.to_string(),
                },
                announcement_channels,
            });
        }

        // Get current bot activity
        let current_activity = bot_state.activity_manager.current().await;

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
