use super::utils::save_json_to_file;
use crate::persistence::DjSettings;
use serenity::model::id::GuildId;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct DjSettingsStore {
    settings: Arc<RwLock<HashMap<GuildId, DjSettings>>>,
    path: PathBuf,
}

impl DjSettingsStore {
    pub fn new(path: PathBuf) -> Self {
        let settings = std::fs::read_to_string(&path)
            .ok()
            .and_then(|contents| serde_json::from_str(&contents).ok())
            .unwrap_or_default();
        Self {
            settings: Arc::new(RwLock::new(settings)),
            path,
        }
    }

    pub async fn get(&self, guild_id: GuildId) -> DjSettings {
        self.settings
            .read()
            .await
            .get(&guild_id)
            .cloned()
            .unwrap_or_default()
    }

    pub async fn update<F>(&self, guild_id: GuildId, f: F) -> super::Result<()>
    where
        F: FnOnce(&mut DjSettings) -> super::Result<()>,
    {
        let mut map = self.settings.write().await;
        // Mutate a clone so a failed `f` leaves the map (and file) untouched.
        let mut entry = map.get(&guild_id).cloned().unwrap_or_default();
        f(&mut entry)?;
        map.insert(guild_id, entry);
        save_json_to_file(&*map, &self.path).await
    }
}
