use crate::persistence::StateStore;
use serenity::model::id::GuildId;
use songbird::{Call, tracks::TrackHandle};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

pub type Data = Arc<BotState>;

pub struct BotState {
    pub voice_connections: RwLock<HashMap<GuildId, Arc<tokio::sync::Mutex<Call>>>>,
    pub track_handles: RwLock<HashMap<GuildId, TrackHandle>>,
    pub audio_file_path: String,
    pub state_store: Arc<dyn StateStore>,
}

impl BotState {
    pub fn new(audio_file_path: String, state_store: Arc<dyn StateStore>) -> Self {
        Self {
            voice_connections: RwLock::new(HashMap::new()),
            track_handles: RwLock::new(HashMap::new()),
            audio_file_path,
            state_store,
        }
    }
}

impl std::fmt::Debug for BotState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BotState")
            .field("audio_file_path", &self.audio_file_path)
            .field("state_store", &"<dyn StateStore>")
            .finish_non_exhaustive()
    }
}
