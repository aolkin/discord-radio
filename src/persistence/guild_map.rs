use super::utils::save_json_to_file;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serenity::model::id::GuildId;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct GuildMapStore<T> {
    values: Arc<RwLock<HashMap<GuildId, T>>>,
    path: PathBuf,
}

impl<T: Clone + Default + Serialize + DeserializeOwned> GuildMapStore<T> {
    pub fn new(path: PathBuf) -> Self {
        let values = std::fs::read_to_string(&path)
            .ok()
            .and_then(|contents| serde_json::from_str(&contents).ok())
            .unwrap_or_default();
        Self {
            values: Arc::new(RwLock::new(values)),
            path,
        }
    }

    pub async fn get(&self, guild_id: GuildId) -> T {
        self.values
            .read()
            .await
            .get(&guild_id)
            .cloned()
            .unwrap_or_default()
    }

    pub async fn update<F>(&self, guild_id: GuildId, f: F) -> super::Result<()>
    where
        F: FnOnce(&mut T) -> super::Result<()>,
    {
        let mut map = self.values.write().await;
        // Mutate a clone so a failed `f` leaves the map (and file) untouched.
        let mut entry = map.get(&guild_id).cloned().unwrap_or_default();
        f(&mut entry)?;
        map.insert(guild_id, entry);
        save_json_to_file(&*map, &self.path).await
    }
}
