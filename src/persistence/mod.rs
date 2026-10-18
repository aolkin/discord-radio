mod dj;
mod file_store;
mod types;
mod utils;

pub use dj::DjSettingsStore;
pub use file_store::FileStore;
pub use types::{
    ActivityState, DJConfigOverrideCategory, DJConfigOverrideSingle, DJState, DjSettings,
    MultiTrackPlaybackState, RegisteredChannel, TrackState,
};

pub(crate) use crate::persistence::types::DJStateSnapshot;
use anyhow::Result;
use async_trait::async_trait;
use serenity::model::id::{ChannelId, GuildId};
use std::collections::HashMap;
use std::path::PathBuf;

#[async_trait]
pub trait StateStore: Send + Sync {
    fn logs_dir(&self) -> PathBuf;
    fn guild_logs_dir(&self, guild_id: u64) -> PathBuf;

    async fn log_member_activity(
        &self,
        guild_id: u64,
        user_id: u64,
        username: &str,
        nickname: Option<&str>,
        action: &str,
        channel_id: Option<u64>,
    ) -> Result<()>;
    async fn log_dj_activity(
        &self,
        guild_id: u64,
        event_type: String,
        details: serde_json::Value,
    ) -> Result<()>;

    async fn save_voice_channel(&self, guild_id: GuildId, channel_id: ChannelId) -> Result<()>;
    async fn load_voice_channels(&self) -> Result<HashMap<GuildId, ChannelId>>;
    async fn remove_voice_channel(&self, guild_id: GuildId) -> Result<()>;

    async fn save_multitrack_playback(
        &self,
        guild_id: GuildId,
        state: &MultiTrackPlaybackState,
    ) -> Result<()>;
    async fn load_multitrack_playbacks(&self) -> Result<HashMap<GuildId, MultiTrackPlaybackState>>;
    #[allow(dead_code)]
    async fn remove_multitrack_playback(&self, guild_id: GuildId) -> Result<()>;

    async fn save_dj_state(&self, guild_id: GuildId, state: &DJStateSnapshot) -> Result<()>;
    async fn load_dj_states(&self) -> Result<HashMap<GuildId, DJState>>;
    async fn remove_dj_state(&self, guild_id: GuildId) -> Result<()>;

    async fn save_registered_channel(&self, channel: &RegisteredChannel) -> Result<()>;
    async fn load_registered_channels(&self) -> Result<Vec<RegisteredChannel>>;
    #[allow(dead_code)]
    async fn remove_registered_channel(&self, channel_id: ChannelId) -> Result<()>;

    async fn save_activity_state(&self, state: Option<&ActivityState>) -> Result<()>;
    async fn load_activity_state(&self) -> Result<Option<ActivityState>>;
}
