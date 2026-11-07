mod file_store;
mod types;
mod utils;

use anyhow::Result;
use async_trait::async_trait;
pub use file_store::FileStore;
use serenity::model::id::{ChannelId, GuildId};
use std::collections::HashMap;
use std::path::PathBuf;
pub use types::{
    ActivityState, DJState, DJStateSnapshot, MultiTrackPlaybackState, RegisteredChannel, TrackState,
};

#[async_trait]
pub trait StateStore: Send + Sync {
    fn guild_logs_dir(&self, guild_id: u64) -> PathBuf;

    async fn save_voice_channel(&self, guild_id: GuildId, channel_id: ChannelId) -> Result<()>;
    async fn load_voice_channels(&self) -> Result<HashMap<GuildId, ChannelId>>;
    async fn remove_voice_channel(&self, guild_id: GuildId) -> Result<()>;

    async fn save_multitrack_playback(
        &self,
        guild_id: GuildId,
        state: &MultiTrackPlaybackState,
    ) -> Result<()>;
    async fn load_multitrack_playbacks(&self) -> Result<HashMap<GuildId, MultiTrackPlaybackState>>;

    async fn save_dj_state(&self, guild_id: GuildId, state: &DJStateSnapshot) -> Result<()>;
    async fn load_dj_states(&self) -> Result<HashMap<GuildId, DJState>>;

    async fn save_registered_channel(&self, channel: &RegisteredChannel) -> Result<()>;
    async fn load_registered_channels(&self) -> Result<Vec<RegisteredChannel>>;

    async fn save_activity_state(&self, state: Option<&ActivityState>) -> Result<()>;
    async fn load_activity_state(&self) -> Result<Option<ActivityState>>;
}
