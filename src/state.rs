use crate::audio::dj::manager::DJManager;
use crate::audio::dj::state_machine::DJState;
use crate::audio::duration::DurationCache;
use crate::audio::processing_thread::AudioProcessor;
use crate::audio::profiles::ProfileManager;
use crate::audio::tracks::TrackManager;
use crate::bucket::FileCache;
use crate::logging::{JsonLogger, guild_logs_dir};
use crate::metrics::MetricsHandle;
use crate::persistence::{DJConfigOverridesStore, StateStore};
use crate::voice_status::{ActivityManager, VoiceChannelStatusManager};
use serde::Serialize;
use serenity::model::id::GuildId;
use songbird::Call;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use tokio::task::JoinHandle;

pub type Data = Arc<BotState>;

#[derive(Clone, Debug)]
pub struct HexPlaybackState {
    pub message: Option<String>,
    pub current_position: usize,
    pub volume: f32,
    pub current_loop: usize,
    pub target_loops: Option<usize>,
    pub status_message: Option<String>, // The obfuscated message pushed to status stack
}

impl HexPlaybackState {
    pub fn stopped() -> Self {
        Self {
            message: None,
            current_position: 0,
            volume: 1.0,
            current_loop: 0,
            target_loops: None,
            status_message: None,
        }
    }

    pub fn playing(
        message: String,
        position: usize,
        volume: f32,
        target_loops: Option<usize>,
        status_message: Option<String>,
    ) -> Self {
        Self {
            message: Some(message),
            current_position: position,
            volume,
            current_loop: 0,
            target_loops,
            status_message,
        }
    }
}

pub struct BotState {
    pub voice_connections: Arc<RwLock<HashMap<GuildId, Arc<Mutex<Call>>>>>,
    pub dj_config_overrides: DJConfigOverridesStore,
    pub track_managers: RwLock<HashMap<GuildId, Arc<Mutex<TrackManager>>>>,
    pub content_path: String,
    pub state_store: Arc<dyn StateStore>,
    pub hex_playback_states: RwLock<HashMap<GuildId, Arc<RwLock<HexPlaybackState>>>>,
    pub hex_playback_tasks: RwLock<HashMap<GuildId, JoinHandle<()>>>,
    pub duration_cache: DurationCache,
    pub audio_processors: RwLock<HashMap<GuildId, Arc<RwLock<AudioProcessor>>>>,
    pub profile_manager: ProfileManager,
    pub dj_managers: RwLock<HashMap<GuildId, Arc<Mutex<DJManager>>>>,
    pub dj_states: RwLock<HashMap<GuildId, Arc<RwLock<DJState>>>>,
    pub voice_status_manager: VoiceChannelStatusManager,
    pub activity_manager: ActivityManager,
    pub shutdown_tx: tokio::sync::broadcast::Sender<String>,
    pub logs_base_path: std::path::PathBuf,
    pub metrics: MetricsHandle,
    #[allow(dead_code)]
    pub file_cache: Arc<FileCache>,
}

impl BotState {
    pub fn new(
        content_path: String,
        dj_config_overrides: DJConfigOverridesStore,
        state_store: Arc<dyn StateStore>,
        shutdown_tx: tokio::sync::broadcast::Sender<String>,
        metrics: MetricsHandle,
        file_cache: Arc<FileCache>,
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

        Self {
            voice_connections: voice_connections.clone(),
            track_managers: RwLock::new(HashMap::new()),
            content_path,
            dj_config_overrides,
            state_store: state_store.clone(),
            hex_playback_states: RwLock::new(HashMap::new()),
            hex_playback_tasks: RwLock::new(HashMap::new()),
            duration_cache: DurationCache::new(),
            audio_processors: RwLock::new(HashMap::new()),
            profile_manager,
            dj_managers: RwLock::new(HashMap::new()),
            dj_states: RwLock::new(HashMap::new()),
            voice_status_manager: VoiceChannelStatusManager::new(voice_connections),
            activity_manager: ActivityManager::new(state_store),
            shutdown_tx,
            logs_base_path,
            metrics,
            file_cache,
        }
    }

    pub fn hex_audio_dir(&self) -> String {
        format!("{}/audio/hex/", self.content_path)
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
        event_type: &str,
        details: serde_json::Value,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
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
            event_type: event_type.to_string(),
            details,
        };

        logger.log(&entry).await
    }
}

impl std::fmt::Debug for BotState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BotState")
            .field("content_path", &self.content_path)
            .field("state_store", &"<dyn StateStore>")
            .finish_non_exhaustive()
    }
}
