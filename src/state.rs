use crate::persistence::StateStore;
use serenity::model::id::GuildId;
use songbird::{Call, tracks::TrackHandle};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;

pub type Data = Arc<BotState>;

pub struct BotState {
    pub voice_connections: RwLock<HashMap<GuildId, Arc<tokio::sync::Mutex<Call>>>>,
    pub track_handles: RwLock<HashMap<GuildId, TrackHandle>>,
    pub hex_audio_dir: String,
    pub state_store: Arc<dyn StateStore>,
    pub message_playback_tokens: RwLock<HashMap<GuildId, CancellationToken>>,
}

impl BotState {
    pub fn new(hex_audio_dir: String, state_store: Arc<dyn StateStore>) -> Self {
        Self {
            voice_connections: RwLock::new(HashMap::new()),
            track_handles: RwLock::new(HashMap::new()),
            hex_audio_dir,
            state_store,
            message_playback_tokens: RwLock::new(HashMap::new()),
        }
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
