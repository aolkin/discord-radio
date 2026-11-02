use crate::audio::dj::config::{ConfiguredPool, DJConfig, LoadedConfig, Pool};
use crate::bucket::FileResolver;
use crate::bucket::registry::{JsonRegistry, RegistryError};
use anyhow::Context;
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::sync::{Arc, Mutex};
use tokio::sync::watch;

pub struct DJConfigRegistry {
    pub configs: JsonRegistry<DJConfig>,
    pub pools: JsonRegistry<Pool>,
    loaded: Mutex<HashMap<String, watch::Sender<Arc<LoadedConfig>>>>,
}

impl DJConfigRegistry {
    pub async fn from_storage(
        configs_path: String,
        pools_path: String,
        file_resolver: FileResolver,
    ) -> Self {
        let registry = Self {
            configs: JsonRegistry::from_storage(configs_path, file_resolver.clone()).await,
            pools: JsonRegistry::from_storage(pools_path, file_resolver).await,
            loaded: Mutex::default(),
        };
        for name in registry.configs.list() {
            registry.publish(&name);
        }
        registry
    }

    pub fn subscribe(&self, name: &str) -> anyhow::Result<watch::Receiver<Arc<LoadedConfig>>> {
        let mut loaded = self.loaded.lock().unwrap();
        if let Some(sender) = loaded.get(name) {
            return Ok(sender.subscribe());
        }
        let config = Arc::new(self.load(name)?);
        Ok(loaded
            .entry(name.to_string())
            .or_insert_with(|| watch::channel(config).0)
            .subscribe())
    }

    pub async fn save_config(&self, name: &str, config: DJConfig) -> Result<(), RegistryError> {
        for pool in config.pools.keys() {
            self.pools.require(pool).map_err(RegistryError::Invalid)?;
        }
        self.configs.save(name, config).await?;
        self.publish(name);
        Ok(())
    }

    pub async fn refresh_config(&self, name: &str) -> Result<Arc<DJConfig>, RegistryError> {
        let config = self.configs.refresh(name).await?;
        self.publish(name);
        Ok(config)
    }

    pub async fn save_pool(&self, name: &str, pool: Pool) -> Result<(), RegistryError> {
        self.pools.save(name, pool).await?;
        self.publish_pool(name);
        Ok(())
    }

    pub async fn refresh_pool(&self, name: &str) -> Result<Arc<Pool>, RegistryError> {
        let pool = self.pools.refresh(name).await?;
        self.publish_pool(name);
        Ok(pool)
    }

    fn publish(&self, name: &str) {
        let mut loaded = self.loaded.lock().unwrap();
        let config = match self.load(name) {
            Ok(config) => Arc::new(config),
            Err(e) => {
                tracing::warn!("Keeping the last loaded copy of DJ config {name:?}: {e:#}");
                return;
            }
        };
        match loaded.entry(name.to_string()) {
            Entry::Occupied(sender) => {
                sender.get().send_replace(config);
            }
            Entry::Vacant(slot) => {
                slot.insert(watch::channel(config).0);
            }
        }
    }

    fn publish_pool(&self, pool: &str) {
        for name in self.configs.list() {
            if self
                .configs
                .get(&name)
                .is_some_and(|config| config.pools.contains_key(pool))
            {
                self.publish(&name);
            }
        }
    }

    fn load(&self, name: &str) -> anyhow::Result<LoadedConfig> {
        let config = self
            .configs
            .get(name)
            .with_context(|| format!("no DJ config {name:?}"))?;
        let pools = config
            .pools
            .iter()
            .map(|(pool_name, pool_config)| {
                let file = self.pools.require(pool_name)?;
                Ok(ConfiguredPool {
                    weight: pool_config.weight.get(),
                    segment_defaults: pool_config.segment_defaults.or(&file.segment_defaults),
                    file,
                })
            })
            .collect::<anyhow::Result<_>>()
            .with_context(|| format!("DJ config {name:?}"))?;
        Ok(LoadedConfig { config, pools })
    }
}
