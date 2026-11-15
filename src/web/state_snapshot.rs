use crate::activity::ActivityEntry;
use crate::audio::dj::state_machine::{ChannelStatus, DJState, PoolProgress};
use crate::audio::tracks::TrackSnapshot;
use crate::state::{Data, GuildRadio};
use serde::{Deserialize, Serialize};
use serenity::model::id::GuildId;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DJStateInfo {
    pub state_type: String,
    pub details: String,
    pub pool: Option<String>,
    pub progress: Option<PoolProgress>,
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
    pub name: Option<String>,
    pub in_voice: bool,
    pub tracks: Vec<TrackSnapshot>,
    pub dj_state: DJStateInfo,
    pub dj_config: String,
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
        let radios: BTreeMap<GuildId, GuildRadio> = bot_state.guild_radios().into_iter().collect();
        let mut guild_ids: BTreeSet<GuildId> = radios.keys().copied().collect();
        if let Some(cache) = bot_state.cache.get() {
            guild_ids.extend(cache.guilds());
        }
        let default_profile_name = bot_state.profile_manager.default_profile().name;

        let mut guilds = Vec::new();
        for guild_id in guild_ids {
            let in_voice = bot_state.get_voice_channel_id(guild_id).await.is_some();
            let guild = match radios.get(&guild_id) {
                Some(radio) => {
                    let tracks = radio.track_manager.get_all_tracks().await;
                    let (profile_name, transitioning) = {
                        let processor = radio.audio_processor.read().await;
                        (
                            processor.current_profile_name().to_string(),
                            processor.is_transitioning(),
                        )
                    };
                    let dj_state = radio.dj_manager.state();
                    let state = dj_state.read().await;
                    guild_state(
                        bot_state,
                        guild_id,
                        in_voice,
                        tracks,
                        &state,
                        profile_name,
                        transitioning,
                    )
                }
                None => guild_state(
                    bot_state,
                    guild_id,
                    in_voice,
                    Vec::new(),
                    &DJState::default(),
                    default_profile_name.clone(),
                    false,
                ),
            };
            guilds.push(guild);
        }

        let current_activity = bot_state.activity_manager.current().await;

        BotSnapshot {
            guilds,
            version: env!("BUILD_RUN_NUMBER").to_string(),
            commit_hash: env!("BUILD_COMMIT_HASH").to_string(),
            current_activity,
        }
    }
}

fn guild_state(
    bot_state: &Data,
    guild_id: GuildId,
    in_voice: bool,
    tracks: Vec<TrackSnapshot>,
    state: &DJState,
    profile_name: String,
    transitioning: bool,
) -> GuildState {
    let cached_guild = bot_state
        .cache
        .get()
        .and_then(|cache| cache.guild(guild_id));
    let profile = state.signal_profile();
    let channel_status = state.channel_status();
    GuildState {
        guild_id: guild_id.to_string(),
        name: cached_guild.as_ref().map(|guild| guild.name.clone()),
        in_voice,
        tracks,
        dj_state: dj_state_to_info(state, bot_state),
        dj_config: state.config_name.clone(),
        audio_profile: AudioProfileInfo {
            name: profile_name,
            expected_name: profile.value.name,
            transitioning,
            source: profile.tier.to_string(),
        },
        channel_status: ChannelStatusInfo {
            status: match channel_status.value {
                ChannelStatus::Unmanaged => None,
                ChannelStatus::Fixed(status) => Some(status),
            },
            source: channel_status.tier.to_string(),
        },
        announcement_channels: state
            .announcement_channels
            .iter()
            .map(|channel| AnnouncementChannelInfo {
                channel_id: channel.to_string(),
                name: cached_guild
                    .as_ref()
                    .and_then(|guild| guild.channels.get(channel).map(|c| c.name.clone())),
            })
            .collect(),
    }
}

fn dj_state_to_info(state: &DJState, bot_state: &Data) -> DJStateInfo {
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
        pool: state.segment.pool.clone(),
        progress: state.pool_progress(bot_state.dj_config_manager),
    }
}
