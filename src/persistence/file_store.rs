use super::{MessagePlaybackState, MultiTrackPlaybackState, ProfileState, Result, StateStore};
use async_trait::async_trait;
use serenity::model::id::{ChannelId, GuildId};
use std::collections::HashMap;
use std::path::PathBuf;
use tokio::fs;
use tokio::io::AsyncWriteExt;

struct PersistedMap<K, V> {
    base_path: PathBuf,
    filename: String,
    _phantom: std::marker::PhantomData<(K, V)>,
}

impl<K, V> PersistedMap<K, V>
where
    K: Eq + std::hash::Hash + serde::Serialize + serde::de::DeserializeOwned,
    V: serde::Serialize + serde::de::DeserializeOwned,
{
    fn new(base_path: PathBuf, filename: impl Into<String>) -> Self {
        Self {
            base_path,
            filename: filename.into(),
            _phantom: std::marker::PhantomData,
        }
    }

    async fn ensure_directory_exists(&self) -> Result<()> {
        fs::create_dir_all(&self.base_path).await?;
        Ok(())
    }

    async fn write(&self, data: &HashMap<K, V>) -> Result<()> {
        self.ensure_directory_exists().await?;

        let file_path = self.base_path.join(&self.filename);
        let temp_path = self.base_path.join(format!("{}.tmp", &self.filename));

        let json = serde_json::to_string_pretty(data)?;

        let mut file = fs::File::create(&temp_path).await?;
        file.write_all(json.as_bytes()).await?;
        file.sync_all().await?;
        drop(file);

        fs::rename(&temp_path, &file_path).await?;

        Ok(())
    }

    async fn read(&self) -> Result<HashMap<K, V>> {
        let file_path = self.base_path.join(&self.filename);

        if !file_path.exists() {
            return Ok(HashMap::new());
        }

        let content = fs::read_to_string(&file_path).await?;
        let data = serde_json::from_str(&content)?;

        Ok(data)
    }

    async fn insert(&self, key: K, value: V) -> Result<()> {
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
        self.read().await
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

    fn message_playbacks(&self) -> PersistedMap<GuildId, MessagePlaybackState> {
        PersistedMap::new(self.base_path.clone(), "message_playbacks.json")
    }

    fn multitrack_playbacks(&self) -> PersistedMap<GuildId, MultiTrackPlaybackState> {
        PersistedMap::new(self.base_path.clone(), "multitrack_playbacks.json")
    }

    fn profile_states(&self) -> PersistedMap<GuildId, ProfileState> {
        PersistedMap::new(self.base_path.clone(), "profile_states.json")
    }
}

#[async_trait]
impl StateStore for FileStore {
    async fn save_voice_channel(&self, guild_id: GuildId, channel_id: ChannelId) -> Result<()> {
        self.voice_channels().insert(guild_id, channel_id).await
    }

    async fn load_voice_channels(&self) -> Result<HashMap<GuildId, ChannelId>> {
        self.voice_channels().load_all().await
    }

    async fn remove_voice_channel(&self, guild_id: GuildId) -> Result<()> {
        self.voice_channels().remove(&guild_id).await
    }

    async fn save_message_playback(
        &self,
        guild_id: GuildId,
        state: &MessagePlaybackState,
    ) -> Result<()> {
        self.message_playbacks()
            .insert(guild_id, state.clone())
            .await
    }

    async fn load_message_playbacks(&self) -> Result<HashMap<GuildId, MessagePlaybackState>> {
        self.message_playbacks().load_all().await
    }

    async fn remove_message_playback(&self, guild_id: GuildId) -> Result<()> {
        self.message_playbacks().remove(&guild_id).await
    }

    async fn save_multitrack_playback(
        &self,
        guild_id: GuildId,
        state: &MultiTrackPlaybackState,
    ) -> Result<()> {
        self.multitrack_playbacks()
            .insert(guild_id, state.clone())
            .await
    }

    async fn load_multitrack_playbacks(&self) -> Result<HashMap<GuildId, MultiTrackPlaybackState>> {
        self.multitrack_playbacks().load_all().await
    }

    async fn remove_multitrack_playback(&self, guild_id: GuildId) -> Result<()> {
        self.multitrack_playbacks().remove(&guild_id).await
    }

    async fn save_profile_state(&self, guild_id: GuildId, state: &ProfileState) -> Result<()> {
        self.profile_states().insert(guild_id, state.clone()).await
    }

    async fn load_profile_states(&self) -> Result<HashMap<GuildId, ProfileState>> {
        self.profile_states().load_all().await
    }
}
