use crate::activity::ActivityManager;
use crate::audio::dj::config::storage::DjConfigManager;
use crate::audio::dj::manager::{DJCommand, DJManager};
use crate::audio::dj::state_machine::DJState;
use crate::audio::duration::DurationCache;
use crate::audio::processing::processor::{AudioProcessor, ProcessorRef};
use crate::audio::profiles::ProfileManager;
use crate::audio::tracks::TrackManager;
use crate::bucket::FileResolver;
use crate::logging::JsonLogger;
use crate::persistence::{DjSettingsStore, StateStore};
use serde::Serialize;
use serenity::all::{ChannelId, UserId};
use serenity::model::id::GuildId;
use songbird::Songbird;
use std::collections::HashMap;
use std::fmt::{Debug, Formatter};
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

pub type Data = &'static BotState;

pub struct BotState {
    pub songbird: Arc<Songbird>,
    pub track_managers: RwLock<HashMap<GuildId, Arc<Mutex<TrackManager>>>>,
    pub state_store: Arc<dyn StateStore>,
    pub dj_settings: DjSettingsStore,
    pub duration_cache: DurationCache,
    pub audio_processors: RwLock<HashMap<GuildId, Arc<RwLock<AudioProcessor>>>>,
    pub profile_manager: ProfileManager,
    pub dj_config_manager: DjConfigManager,
    pub dj_managers: RwLock<HashMap<GuildId, Arc<Mutex<DJManager>>>>,
    pub activity_manager: ActivityManager,
    pub file_resolver: FileResolver,
    pub bot_user_id: UserId,
}

impl BotState {
    pub fn new(
        file_resolver: FileResolver,
        state_store: Arc<dyn StateStore>,
        dj_settings: DjSettingsStore,
        profile_manager: ProfileManager,
        dj_config_manager: DjConfigManager,
        bot_user_id: UserId,
        songbird: Arc<Songbird>,
    ) -> Self {
        Self {
            songbird,
            track_managers: RwLock::new(HashMap::new()),
            duration_cache: DurationCache::new(file_resolver.clone()),
            state_store: state_store.clone(),
            dj_settings,
            audio_processors: RwLock::new(HashMap::new()),
            profile_manager,
            dj_config_manager,
            dj_managers: RwLock::new(HashMap::new()),
            activity_manager: ActivityManager::new(state_store),
            file_resolver,
            bot_user_id,
        }
    }

    pub async fn audio_processor(&self, guild_id: GuildId) -> ProcessorRef {
        let processors = self.audio_processors.read().await;
        if let Some(processor) = processors.get(&guild_id) {
            return processor.clone();
        }
        drop(processors);

        let mut processors = self.audio_processors.write().await;
        // Double-check in case another task created it while we were waiting
        processors
            .entry(guild_id)
            .or_insert_with(|| {
                let initial_profile = self.profile_manager.default_profile();
                Arc::new(RwLock::new(AudioProcessor::new(initial_profile)))
            })
            .clone()
    }

    pub async fn dj_state(&self, guild_id: GuildId) -> Option<Arc<RwLock<DJState>>> {
        let manager = self.dj_managers.read().await.get(&guild_id)?.clone();
        let state = manager.lock().await.state.clone();
        Some(state)
    }

    pub async fn send_dj_command(
        &self,
        guild_id: GuildId,
        command: DJCommand,
    ) -> anyhow::Result<bool> {
        let Some(manager) = self.dj_managers.read().await.get(&guild_id).cloned() else {
            return Ok(false);
        };
        manager.lock().await.send(command).await?;
        Ok(true)
    }

    /// Get the current voice channel ID for a guild
    pub async fn get_voice_channel_id(&self, guild_id: GuildId) -> Option<ChannelId> {
        let call_lock = self.songbird.get(guild_id)?;
        let call = call_lock.lock().await;
        call.current_channel().map(|id| ChannelId::new(id.0.get()))
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
