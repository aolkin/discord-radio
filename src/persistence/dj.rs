use super::utils::save_json_to_file;
use crate::persistence::DjSettings;
use anyhow::Context;
use serenity::model::id::GuildId;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct DjSettingsStore {
    settings: Arc<RwLock<HashMap<GuildId, DjSettings>>>,
    path: PathBuf,
}

impl DjSettingsStore {
    pub fn new(path: PathBuf) -> Self {
        let settings = Self::load(&path).unwrap_or_else(|e| {
            tracing::warn!("Starting with empty DJ settings: {e:#}");
            HashMap::new()
        });
        Self {
            settings: Arc::new(RwLock::new(settings)),
            path,
        }
    }

    fn load(path: &Path) -> anyhow::Result<HashMap<GuildId, DjSettings>> {
        if !path.exists() {
            return Ok(HashMap::new());
        }
        let contents =
            std::fs::read_to_string(path).with_context(|| format!("reading {path:?}"))?;
        serde_json::from_str(&contents).with_context(|| format!("parsing {path:?}"))
    }

    pub async fn get(&self, guild_id: GuildId) -> DjSettings {
        self.settings
            .read()
            .await
            .get(&guild_id)
            .cloned()
            .unwrap_or_default()
    }

    pub async fn update<F>(&self, guild_id: GuildId, f: F) -> anyhow::Result<()>
    where
        F: FnOnce(&mut DjSettings) -> anyhow::Result<()>,
    {
        let mut map = self.settings.write().await;
        // Mutate a clone so a failed `f` leaves the map (and file) untouched.
        let mut entry = map.get(&guild_id).cloned().unwrap_or_default();
        f(&mut entry)?;
        map.insert(guild_id, entry);
        save_json_to_file(&*map, &self.path).await
    }
}
