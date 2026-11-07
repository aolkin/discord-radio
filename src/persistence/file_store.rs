use super::utils::{read_json, write_json};
use super::{DJState, DJStateSnapshot, MultiTrackPlaybackState, RegisteredChannel, StateStore};
use anyhow::Result;
use async_trait::async_trait;
use serde::Serialize;
use serenity::model::id::{ChannelId, GuildId};
use std::collections::HashMap;
use std::hash::Hash;
use std::marker::PhantomData;
use std::path::PathBuf;

struct PersistedMap<K, V> {
    base_path: PathBuf,
    filename: String,
    _phantom: PhantomData<(K, V)>,
}

impl<K, V> PersistedMap<K, V>
where
    K: Eq + Hash + serde::Serialize + serde::de::DeserializeOwned,
    V: serde::de::DeserializeOwned,
{
    fn new(base_path: PathBuf, filename: impl Into<String>) -> Self {
        Self {
            base_path,
            filename: filename.into(),
            _phantom: PhantomData,
        }
    }

    fn path(&self) -> PathBuf {
        self.base_path.join(&self.filename)
    }

    async fn write(&self, data: &HashMap<K, serde_json::Value>) -> Result<()> {
        write_json(data, &self.path()).await
    }

    async fn read(&self) -> Result<HashMap<K, serde_json::Value>> {
        read_json(&self.path()).await
    }

    async fn insert<T: Serialize>(&self, key: K, value: T) -> Result<()> {
        let value = serde_json::to_value(&value)?;
        let mut map = self.read().await?;
        map.insert(key, value);
        self.write(&map).await
    }

    async fn remove(&self, key: &K) -> Result<()> {
        let mut map = self.read().await?;
        map.remove(key);
        self.write(&map).await
    }

    async fn load_all(&self) -> Result<HashMap<K, V>> {
        self.read().await.map(|values| {
            values
                .into_iter()
                .filter_map(|(k, v)| V::deserialize(&v).ok().map(|v| (k, v)))
                .collect()
        })
    }
}

pub struct FileStore {
    base_path: PathBuf,
}

impl FileStore {
    pub fn new(base_path: PathBuf) -> Self {
        Self { base_path }
    }

    fn voice_channels(&self) -> PersistedMap<GuildId, ChannelId> {
        PersistedMap::new(self.base_path.clone(), "voice_channels.json")
    }

    fn multitrack_playbacks(&self) -> PersistedMap<GuildId, MultiTrackPlaybackState> {
        PersistedMap::new(self.base_path.clone(), "multitrack_playbacks.json")
    }

    fn dj_states(&self) -> PersistedMap<GuildId, DJState> {
        PersistedMap::new(self.base_path.clone(), "dj_states.json")
    }

    fn registered_channels_path(&self) -> PathBuf {
        self.base_path.join("registered_channels.json")
    }

    fn activity_state_path(&self) -> PathBuf {
        self.base_path.join("activity_state.json")
    }
}

#[async_trait]
impl StateStore for FileStore {
    fn guild_logs_dir(&self, guild_id: u64) -> PathBuf {
        crate::logging::guild_logs_dir(&self.base_path, guild_id)
    }

    async fn save_voice_channel(&self, guild_id: GuildId, channel_id: ChannelId) -> Result<()> {
        self.voice_channels().insert(guild_id, &channel_id).await
    }

    async fn load_voice_channels(&self) -> Result<HashMap<GuildId, ChannelId>> {
        self.voice_channels().load_all().await
    }

    async fn remove_voice_channel(&self, guild_id: GuildId) -> Result<()> {
        self.voice_channels().remove(&guild_id).await
    }

    async fn save_multitrack_playback(
        &self,
        guild_id: GuildId,
        state: &MultiTrackPlaybackState,
    ) -> Result<()> {
        self.multitrack_playbacks().insert(guild_id, state).await
    }

    async fn load_multitrack_playbacks(&self) -> Result<HashMap<GuildId, MultiTrackPlaybackState>> {
        self.multitrack_playbacks().load_all().await
    }

    async fn save_dj_state(&self, guild_id: GuildId, state: &DJStateSnapshot) -> Result<()> {
        self.dj_states().insert(guild_id, state).await
    }

    async fn load_dj_states(&self) -> Result<HashMap<GuildId, DJState>> {
        self.dj_states().load_all().await
    }

    async fn save_registered_channel(&self, channel: &RegisteredChannel) -> Result<()> {
        let mut channels = self.load_registered_channels().await?;
        channels.retain(|c| c.channel_id != channel.channel_id);
        channels.push(channel.clone());
        write_json(&channels, &self.registered_channels_path()).await
    }

    async fn load_registered_channels(&self) -> Result<Vec<RegisteredChannel>> {
        read_json(&self.registered_channels_path()).await
    }

    async fn save_activity_state(&self, state: Option<&super::ActivityState>) -> Result<()> {
        write_json(&state, &self.activity_state_path()).await
    }

    async fn load_activity_state(&self) -> Result<Option<super::ActivityState>> {
        read_json(&self.activity_state_path()).await
    }
}
