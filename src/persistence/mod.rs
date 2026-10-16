mod dj;
mod file_store;
mod types;
mod utils;

pub use dj::DjSettingsStore;
pub use file_store::FileStore;
pub use types::{
    ActivityState, DJState, DjSettings, MultiTrackPlaybackState, RegisteredChannel, TrackState,
};

pub(crate) use crate::persistence::types::DJStateSnapshot;
use anyhow::Result;
use async_trait::async_trait;
use serenity::model::id::{ChannelId, GuildId};
use std::collections::HashMap;
use std::path::Path;

#[async_trait]
pub trait StateStore: Send + Sync {
    /// Get the base path for this state store
    fn base_path(&self) -> &Path;

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
