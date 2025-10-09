use serenity::model::id::GuildId;
use songbird::{Call, tracks::TrackHandle};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

pub type Data = Arc<BotState>;

#[derive(Debug)]
pub struct BotState {
    pub voice_connections: RwLock<HashMap<GuildId, Arc<tokio::sync::Mutex<Call>>>>,
    pub track_handles: RwLock<HashMap<GuildId, TrackHandle>>,
    pub audio_file_path: String,
}

impl BotState {
    pub fn new(audio_file_path: String) -> Self {
        Self {
            voice_connections: RwLock::new(HashMap::new()),
            track_handles: RwLock::new(HashMap::new()),
            audio_file_path,
        }
    }
}
