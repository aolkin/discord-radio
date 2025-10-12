mod dj;
mod file_store;
mod types;

pub use file_store::FileStore;
pub use types::{
    DJState, DJStateMachineState, MessagePlaybackState, MultiTrackPlaybackState, ProfileState,
    TrackState,
};

use async_trait::async_trait;
use serenity::model::id::{ChannelId, GuildId};
use std::collections::HashMap;

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[async_trait]
pub trait StateStore: Send + Sync {
    async fn save_voice_channel(&self, guild_id: GuildId, channel_id: ChannelId) -> Result<()>;
    async fn load_voice_channels(&self) -> Result<HashMap<GuildId, ChannelId>>;
    async fn remove_voice_channel(&self, guild_id: GuildId) -> Result<()>;

    async fn save_message_playback(
        &self,
        guild_id: GuildId,
        state: &MessagePlaybackState,
    ) -> Result<()>;
    async fn load_message_playbacks(&self) -> Result<HashMap<GuildId, MessagePlaybackState>>;
    async fn remove_message_playback(&self, guild_id: GuildId) -> Result<()>;

    async fn save_multitrack_playback(
        &self,
        guild_id: GuildId,
        state: &MultiTrackPlaybackState,
    ) -> Result<()>;
    async fn load_multitrack_playbacks(&self) -> Result<HashMap<GuildId, MultiTrackPlaybackState>>;
    async fn remove_multitrack_playback(&self, guild_id: GuildId) -> Result<()>;

    async fn save_profile_state(&self, guild_id: GuildId, state: &ProfileState) -> Result<()>;
    async fn load_profile_states(&self) -> Result<HashMap<GuildId, ProfileState>>;

    async fn save_dj_state(&self, guild_id: GuildId, state: &DJState) -> Result<()>;
    async fn load_dj_states(&self) -> Result<HashMap<GuildId, DJState>>;
    async fn remove_dj_state(&self, guild_id: GuildId) -> Result<()>;
}
