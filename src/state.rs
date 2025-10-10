use crate::audio::duration::DurationCache;
use crate::audio::processing_thread::AudioProcessor;
use crate::audio::tracks::TrackManager;
use crate::persistence::StateStore;
use serenity::model::id::GuildId;
use songbird::Call;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::task::JoinHandle;

pub type Data = Arc<BotState>;

#[derive(Clone, Debug)]
pub struct HexPlaybackState {
    pub message: Option<String>,
    pub current_position: usize,
    pub volume: f32,
}

impl HexPlaybackState {
    pub fn stopped() -> Self {
        Self {
            message: None,
            current_position: 0,
            volume: 1.0,
        }
    }

    pub fn playing(message: String, position: usize, volume: f32) -> Self {
        Self {
            message: Some(message),
            current_position: position,
            volume,
        }
    }
}

pub struct BotState {
    pub voice_connections: RwLock<HashMap<GuildId, Arc<tokio::sync::Mutex<Call>>>>,
    pub track_managers: RwLock<HashMap<GuildId, Arc<tokio::sync::Mutex<TrackManager>>>>,
    pub hex_audio_dir: String,
    pub state_store: Arc<dyn StateStore>,
    pub hex_playback_states: RwLock<HashMap<GuildId, Arc<RwLock<HexPlaybackState>>>>,
    pub hex_playback_tasks: RwLock<HashMap<GuildId, JoinHandle<()>>>,
    pub duration_cache: DurationCache,
    pub audio_processors: RwLock<HashMap<GuildId, Arc<RwLock<AudioProcessor>>>>,
    pub audio_processor_tasks: RwLock<HashMap<GuildId, JoinHandle<Result<(), Box<dyn std::error::Error + Send + Sync>>>>>,
}

impl BotState {
    pub fn new(hex_audio_dir: String, state_store: Arc<dyn StateStore>) -> Self {
        Self {
            voice_connections: RwLock::new(HashMap::new()),
            track_managers: RwLock::new(HashMap::new()),
            hex_audio_dir,
            state_store,
            hex_playback_states: RwLock::new(HashMap::new()),
            hex_playback_tasks: RwLock::new(HashMap::new()),
            duration_cache: DurationCache::new(),
            audio_processors: RwLock::new(HashMap::new()),
            audio_processor_tasks: RwLock::new(HashMap::new()),
        }
    }

    pub fn audio_profiles_dir(&self) -> String {
        "audio_profiles".to_string()
    }
}

impl std::fmt::Debug for BotState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BotState")
            .field("hex_audio_dir", &self.hex_audio_dir)
            .field("state_store", &"<dyn StateStore>")
            .finish_non_exhaustive()
    }
}
