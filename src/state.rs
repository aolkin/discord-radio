use crate::audio::dj::manager::DJManager;
use crate::audio::dj::state_machine::DJState;
use crate::audio::duration::DurationCache;
use crate::audio::processing::processor::{AudioProcessor, ProcessorRef};
use crate::audio::profiles::ProfileManager;
use crate::audio::tracks::TrackManager;
use crate::bucket::{FileResolver, ObjectStore};
use crate::logging::{JsonLogger, guild_logs_dir};
use crate::persistence::{DjSettingsStore, StateStore};
use crate::voice_status::ActivityManager;
use serde::Serialize;
use serenity::all::ChannelId;
use serenity::model::id::GuildId;
use songbird::Call;
use std::collections::HashMap;
use std::fmt::{Debug, Formatter};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

pub type Data = &'static BotState;

pub struct BotState {
    pub voice_connections: Arc<RwLock<HashMap<GuildId, Arc<Mutex<Call>>>>>,
    pub track_managers: RwLock<HashMap<GuildId, Arc<Mutex<TrackManager>>>>,
    pub content_path: String,
    pub state_store: Arc<dyn StateStore>,
    pub dj_settings: DjSettingsStore,
    pub duration_cache: DurationCache,
    pub audio_processors: RwLock<HashMap<GuildId, Arc<RwLock<AudioProcessor>>>>,
    pub profile_manager: ProfileManager,
    pub dj_managers: RwLock<HashMap<GuildId, Arc<Mutex<DJManager>>>>,
    pub dj_states: RwLock<HashMap<GuildId, Arc<RwLock<DJState>>>>,
    pub activity_manager: ActivityManager,
    pub shutdown_tx: tokio::sync::broadcast::Sender<String>,
    pub logs_base_path: PathBuf,
    pub file_resolver: FileResolver,
}

impl BotState {
    pub fn new(
        content_path: String,
        state_store: Arc<dyn StateStore>,
        dj_settings: DjSettingsStore,
        shutdown_tx: tokio::sync::broadcast::Sender<String>,
        file_cache: Arc<ObjectStore>,
    ) -> Self {
        let profiles_dir = "audio_profiles";
        let mut profile_manager = ProfileManager::new(profiles_dir);

        if let Err(e) = profile_manager.load_all() {
            tracing::warn!("Failed to load audio profiles at startup: {}", e);
        } else {
            tracing::info!(
                "Loaded {} audio profiles",
                profile_manager.list_profiles().len()
            );
        }

        let voice_connections = Arc::new(RwLock::new(HashMap::new()));

        // Get logs base path from state store path
        let logs_base_path = state_store.base_path().to_path_buf();

        let file_resolver = FileResolver::new(content_path.clone(), file_cache.clone());

        Self {
            voice_connections: voice_connections.clone(),
            track_managers: RwLock::new(HashMap::new()),
            duration_cache: DurationCache::new(file_resolver.clone()),
            content_path,
            state_store: state_store.clone(),
            dj_settings,
            audio_processors: RwLock::new(HashMap::new()),
            profile_manager,
            dj_managers: RwLock::new(HashMap::new()),
            dj_states: RwLock::new(HashMap::new()),
            activity_manager: ActivityManager::new(state_store),
            shutdown_tx,
            logs_base_path,
            file_resolver,
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
                let initial_profile = crate::audio::tracks::load_default_profile(self);
                Arc::new(RwLock::new(AudioProcessor::new(initial_profile)))
            })
            .clone()
    }

    pub async fn dj_manager(&self, guild_id: GuildId) -> Arc<Mutex<DJManager>> {
        let managers = self.dj_managers.read().await;
        if let Some(manager) = managers.get(&guild_id) {
            return manager.clone();
        }
        drop(managers);

        let mut managers = self.dj_managers.write().await;
        managers
            .entry(guild_id)
            .or_insert_with(|| Arc::new(Mutex::new(DJManager::new(guild_id))))
            .clone()
    }

    /// Get the current voice channel ID for a guild
    pub async fn get_voice_channel_id(&self, guild_id: GuildId) -> Option<ChannelId> {
        let voice_connections = self.voice_connections.read().await;
        if let Some(call_lock) = voice_connections.get(&guild_id) {
            let call = call_lock.lock().await;
            call.current_channel().map(|id| ChannelId::new(id.0.get()))
        } else {
            None
        }
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
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
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

        let log_path = guild_logs_dir(&self.logs_base_path, guild_id).join("members.jsonl");
        let logger = JsonLogger::new(log_path);

        let entry = MemberActivityEntry {
            timestamp: chrono::Utc::now().to_rfc3339(),
            user_id,
            username: username.to_string(),
            nickname: nickname.map(|s| s.to_string()),
            action: action.to_string(),
            channel_id,
        };

        logger.log(&entry).await
    }

    /// Log a DJ state transition event
    pub async fn log_dj_activity(
        &self,
        guild_id: u64,
        event_type: String,
        details: serde_json::Value,
    ) -> bool {
        #[derive(Serialize)]
        struct DJActivityEntry {
            timestamp: String,
            event_type: String,
            details: serde_json::Value,
        }

        let log_path = guild_logs_dir(&self.logs_base_path, guild_id).join("dj.jsonl");
        let logger = JsonLogger::new(log_path);

        let entry = DJActivityEntry {
            timestamp: chrono::Utc::now().to_rfc3339(),
            event_type,
            details,
        };

        logger
            .log(&entry)
            .await
            .inspect_err(|e| {
                tracing::error!("Failed to log DJ state transition: {}", e);
            })
            .is_ok()
    }
}

impl Debug for BotState {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BotState")
            .field("content_path", &self.content_path)
            .field("state_store", &"<dyn StateStore>")
            .finish_non_exhaustive()
    }
}
