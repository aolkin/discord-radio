use crate::audio::dj::manager::DJManager;
use crate::audio::dj::state_machine::DJState;
use crate::audio::duration::DurationCache;
use crate::audio::processing_thread::AudioProcessor;
use crate::audio::profiles::ProfileManager;
use crate::audio::tracks::TrackManager;
use crate::persistence::StateStore;
use crate::voice_status::VoiceChannelStatusManager;
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
    pub shutdown_tx: tokio::sync::broadcast::Sender<String>,
}

impl BotState {
    pub fn new(
        content_path: String,
        state_store: Arc<dyn StateStore>,
        shutdown_tx: tokio::sync::broadcast::Sender<String>,
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

        Self {
            voice_connections: voice_connections.clone(),
            track_managers: RwLock::new(HashMap::new()),
            content_path,
            state_store,
            hex_playback_states: RwLock::new(HashMap::new()),
            hex_playback_tasks: RwLock::new(HashMap::new()),
            duration_cache: DurationCache::new(),
            audio_processors: RwLock::new(HashMap::new()),
            profile_manager,
            dj_managers: RwLock::new(HashMap::new()),
            dj_states: RwLock::new(HashMap::new()),
            voice_status_manager: VoiceChannelStatusManager::new(voice_connections),
            shutdown_tx,
        }
    }

    pub fn audio_profiles_dir(&self) -> String {
        "audio_profiles".to_string()
    }

    pub fn hex_audio_dir(&self) -> String {
        format!("{}/audio/hex/", self.content_path)
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
