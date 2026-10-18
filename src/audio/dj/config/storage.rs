use crate::audio::dj::config::DJConfig;
use crate::audio::profiles::is_valid_name;
use crate::bucket::FileResolver;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

#[derive(Debug, thiserror::Error)]
pub enum DjConfigStorageError {
    #[error("a DJ config name may contain only letters, digits, '-' and '_'")]
    InvalidName,
    #[error(transparent)]
    Storage(#[from] anyhow::Error),
    #[error("parsing DJ config: {0:#}")]
    Parse(anyhow::Error),
}

pub struct DjConfigManager {
    configs: RwLock<HashMap<String, Arc<DJConfig>>>,
    configs_path: String,
    file_resolver: FileResolver,
}

impl DjConfigManager {
    /// Loads every DJ config under `configs_path` locally and in object
    /// storage. A config in object storage replaces a local config of the
    /// same name.
    pub async fn from_storage(configs_path: String, file_resolver: FileResolver) -> Self {
        let mut configs = HashMap::new();

        let filenames = file_resolver.list(&configs_path).await;
        for filename in filenames.iter().filter(|f| f.ends_with(".json")) {
            let parsed = file_resolver
                .resolve_contents(filename)
                .await
                .and_then(|bytes| DJConfig::parse(&bytes));
            match parsed {
                Ok(config) => {
                    tracing::info!("Loaded DJ config {} from {filename}", config.name);
                    configs.insert(config.name.clone(), Arc::new(config));
                }
                Err(e) => tracing::warn!("Failed to load DJ config from {filename}: {e:#}"),
            }
        }

        if configs.is_empty() {
            tracing::warn!("No DJ configs loaded");
        } else {
            tracing::info!("Loaded {} DJ configs", configs.len());
        }

        Self {
            configs: RwLock::new(configs),
            configs_path,
            file_resolver,
        }
    }

    fn remote_key(&self, name: &str) -> Result<String, DjConfigStorageError> {
        if !is_valid_name(name) {
            return Err(DjConfigStorageError::InvalidName);
        }
        Ok(format!("{}{name}.json", self.configs_path))
    }

    pub fn get(&self, name: &str) -> Option<Arc<DJConfig>> {
        self.configs.read().unwrap().get(name).cloned()
    }

    pub fn list(&self) -> Vec<String> {
        self.configs.read().unwrap().keys().cloned().collect()
    }

    /// Writes `config` to object storage under its name, replacing any config
    /// of the same name.
    pub async fn save(&self, config: DJConfig) -> Result<(), DjConfigStorageError> {
        let key = self.remote_key(&config.name)?;
        let bytes = serde_json::to_vec_pretty(&config).expect("a DJConfig always serializes");
        self.file_resolver
            .bucket()?
            .upload(&key, &bytes, "application/json")
            .await?;

        self.configs
            .write()
            .unwrap()
            .insert(config.name.clone(), Arc::new(config));
        Ok(())
    }

    /// Re-fetches the config named `name` from object storage and replaces
    /// the in-memory copy with it.
    pub async fn refresh(&self, name: &str) -> Result<Arc<DJConfig>, DjConfigStorageError> {
        let key = self.remote_key(name)?;
        let bytes = self.file_resolver.bucket()?.fetch_bytes(&key).await?;
        let config = Arc::new(DJConfig::parse(&bytes).map_err(DjConfigStorageError::Parse)?);
        self.configs
            .write()
            .unwrap()
            .insert(config.name.clone(), config.clone());
        Ok(config)
    }
}
