use crate::activity::ActivityManager;
use crate::audio::connection::connect_voice_channel;
use crate::audio::dj::config::registry::DJConfigRegistry;
use crate::audio::dj::manager::DJManager;
use crate::audio::duration::DurationCache;
use crate::audio::processing::processor::{AudioProcessor, ProcessorRef};
use crate::audio::profiles::SignalProfile;
use crate::audio::tracks::TrackManager;
use crate::bucket::FileResolver;
use crate::bucket::registry::JsonRegistry;
use crate::logging::JsonLogger;
use crate::persistence::StateStore;
use anyhow::Context as _;
use serde::Serialize;
use serenity::all::{ChannelId, UserId};
use serenity::model::id::GuildId;
use songbird::Songbird;
use std::collections::HashMap;
use std::fmt::{Debug, Formatter};
use std::sync::{Arc, Mutex as StdMutex, OnceLock};
use tokio::sync::broadcast::Receiver;
use tokio::sync::{Mutex, RwLock};

pub type Data = &'static BotState;

#[derive(Debug, thiserror::Error)]
#[error("guild {0} is not active")]
pub struct GuildInactive(pub GuildId);

#[derive(Clone)]
pub struct GuildRadio {
    pub audio_processor: ProcessorRef,
    pub track_manager: Arc<TrackManager>,
    pub dj_manager: Arc<DJManager>,
}

pub struct BotState {
    pub songbird: Arc<Songbird>,
    pub state_store: Arc<dyn StateStore>,
    pub duration_cache: DurationCache,
    guilds: StdMutex<HashMap<GuildId, GuildRadio>>,
    lifecycles: StdMutex<HashMap<GuildId, Arc<Mutex<()>>>>,
    pub profile_manager: JsonRegistry<SignalProfile>,
    pub dj_config_manager: &'static DJConfigRegistry,
    pub activity_manager: ActivityManager,
    pub file_resolver: FileResolver,
    pub hex_audio_folder: String,
    pub bot_user_id: UserId,
    pub cache: OnceLock<Arc<serenity::cache::Cache>>,
}

impl BotState {
    pub fn new(
        file_resolver: FileResolver,
        hex_audio_folder: String,
        state_store: Arc<dyn StateStore>,
        profile_manager: JsonRegistry<SignalProfile>,
        dj_config_manager: &'static DJConfigRegistry,
        bot_user_id: UserId,
        songbird: Arc<Songbird>,
    ) -> Self {
        Self {
            songbird,
            duration_cache: DurationCache::new(file_resolver.clone()),
            state_store: state_store.clone(),
            guilds: StdMutex::new(HashMap::new()),
            lifecycles: StdMutex::new(HashMap::new()),
            profile_manager,
            dj_config_manager,
            activity_manager: ActivityManager::new(state_store),
            file_resolver,
            hex_audio_folder,
            bot_user_id,
            cache: OnceLock::new(),
        }
    }

    fn lifecycle(&self, guild_id: GuildId) -> Arc<Mutex<()>> {
        self.lifecycles
            .lock()
            .unwrap()
            .entry(guild_id)
            .or_default()
            .clone()
    }

    pub fn radio(&self, guild_id: GuildId) -> Option<GuildRadio> {
        self.guilds.lock().unwrap().get(&guild_id).cloned()
    }

    pub fn require_radio(&self, guild_id: GuildId) -> Result<GuildRadio, GuildInactive> {
        self.radio(guild_id).ok_or(GuildInactive(guild_id))
    }

    /// Every guild that has radio state, ordered by guild id.
    pub fn guild_radios(&self) -> Vec<(GuildId, GuildRadio)> {
        let mut guilds: Vec<_> = self
            .guilds
            .lock()
            .unwrap()
            .iter()
            .map(|(id, radio)| (*id, radio.clone()))
            .collect();
        guilds.sort_by_key(|(id, _)| *id);
        guilds
    }

    pub async fn opus_stream(&self, guild_id: GuildId) -> Option<Receiver<Vec<u8>>> {
        let audio_processor = self.radio(guild_id)?.audio_processor;
        let receiver = audio_processor.read().await.stream_opus();
        Some(receiver)
    }

    /// Get the current voice channel ID for a guild
    pub async fn get_voice_channel_id(&self, guild_id: GuildId) -> Option<ChannelId> {
        let call_lock = self.songbird.get(guild_id)?;
        let call = call_lock.lock().await;
        call.current_channel().map(|id| ChannelId::new(id.0.get()))
    }

    pub async fn join_voice_channel(
        &'static self,
        guild_id: GuildId,
        channel_id: ChannelId,
    ) -> anyhow::Result<()> {
        let lifecycle = self.lifecycle(guild_id);
        let _lifecycle = lifecycle.lock().await;
        let (radio, loaded) = self.load_locked(guild_id).await?;
        if let Err(e) = connect_voice_channel(
            &self.songbird,
            guild_id,
            channel_id,
            radio.audio_processor.clone(),
        )
        .await
        {
            if loaded {
                self.unload_locked(guild_id).await;
            }
            return Err(e);
        }
        tracing::info!("Joined voice channel {}", channel_id);
        if let Err(e) = self
            .state_store
            .save_voice_channel(guild_id, channel_id)
            .await
        {
            tracing::error!("Failed to save voice channel: {e:#}");
        }
        Ok(())
    }

    pub async fn load(&'static self, guild_id: GuildId) -> anyhow::Result<bool> {
        let lifecycle = self.lifecycle(guild_id);
        let _lifecycle = lifecycle.lock().await;
        let (_, loaded) = self.load_locked(guild_id).await?;
        Ok(loaded)
    }

    async fn load_locked(&'static self, guild_id: GuildId) -> anyhow::Result<(GuildRadio, bool)> {
        if let Some(radio) = self.radio(guild_id) {
            return Ok((radio, false));
        }
        let audio_processor = Arc::new(RwLock::new(AudioProcessor::new(
            self.profile_manager.default_profile(),
        )));
        let dj_manager = Arc::new(DJManager::new(guild_id, self, audio_processor.clone()));
        dj_manager.start().await.context("starting the DJ")?;
        let radio = GuildRadio {
            track_manager: Arc::new(TrackManager::new(guild_id, self, audio_processor.clone())),
            audio_processor,
            dj_manager,
        };
        self.guilds.lock().unwrap().insert(guild_id, radio.clone());
        tracing::info!("Loaded guild {guild_id}");
        Ok((radio, true))
    }

    pub async fn leave_voice(&self, guild_id: GuildId) {
        let lifecycle = self.lifecycle(guild_id);
        let _lifecycle = lifecycle.lock().await;
        self.leave_voice_locked(guild_id).await;
    }

    async fn leave_voice_locked(&self, guild_id: GuildId) {
        if let Err(e) = self.state_store.remove_voice_channel(guild_id).await {
            tracing::error!("Failed to remove saved voice channel: {e:#}");
        }
        if self.songbird.get(guild_id).is_some()
            && let Err(e) = self.songbird.remove(guild_id).await
        {
            tracing::error!("Failed to remove voice connection: {e:#}");
        }
    }

    async fn unload_locked(&self, guild_id: GuildId) {
        self.leave_voice_locked(guild_id).await;
        let Some(radio) = self.guilds.lock().unwrap().remove(&guild_id) else {
            return;
        };
        radio.dj_manager.stop().await;
        radio.track_manager.clear().await;
        tracing::info!("Unloaded guild {guild_id}");
    }

    pub async fn saved_voice_channel(&self, guild_id: GuildId) -> Option<ChannelId> {
        match self.state_store.load_voice_channels().await {
            Ok(channels) => channels.get(&guild_id).copied(),
            Err(e) => {
                tracing::error!("Failed to load saved voice channels: {e:#}");
                None
            }
        }
    }

    /// Count the guilds with a Call that is in a voice channel
    pub async fn voice_connection_count(&self) -> usize {
        let mut count = 0;
        for (_, call_lock) in self.songbird.iter() {
            if call_lock.lock().await.current_channel().is_some() {
                count += 1;
            }
        }
        count
    }

    /// Log a member activity event
    pub async fn log_member_activity(
        &self,
        guild_id: u64,
        user_id: u64,
        username: &str,
        nickname: Option<&str>,
        action: &str,
        channel_id: Option<u64>,
    ) {
        #[derive(Serialize)]
        struct MemberActivityEntry {
            timestamp: String,
            user_id: u64,
            username: String,
            #[serde(skip_serializing_if = "Option::is_none")]
            nickname: Option<String>,
            action: String,
            #[serde(skip_serializing_if = "Option::is_none")]
            channel_id: Option<u64>,
        }

        let log_path = self
            .state_store
            .guild_logs_dir(guild_id)
            .join("members.jsonl");
        let logger = JsonLogger::new(log_path);

        let entry = MemberActivityEntry {
            timestamp: chrono::Utc::now().to_rfc3339(),
            user_id,
            username: username.to_string(),
            nickname: nickname.map(|s| s.to_string()),
            action: action.to_string(),
            channel_id,
        };

        if let Err(e) = logger.log(&entry).await {
            tracing::error!("{e:#}");
        }
    }

    /// Log a DJ state transition event
    pub async fn log_dj_activity(
        &self,
        guild_id: u64,
        event_type: String,
        details: serde_json::Value,
    ) {
        #[derive(Serialize)]
        struct DJActivityEntry {
            timestamp: String,
            event_type: String,
            details: serde_json::Value,
        }

        let log_path = self.state_store.guild_logs_dir(guild_id).join("dj.jsonl");
        let logger = JsonLogger::new(log_path);

        let entry = DJActivityEntry {
            timestamp: chrono::Utc::now().to_rfc3339(),
            event_type,
            details,
        };

        if let Err(e) = logger.log(&entry).await {
            tracing::error!("{e:#}");
        }
    }
}

impl Debug for BotState {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BotState")
            .field("content_path", &self.file_resolver.content_path())
            .field("state_store", &"<dyn StateStore>")
            .finish_non_exhaustive()
    }
}
