use crate::audio::dj::config::{ConfiguredPool, DJConfig, LoadedConfig, Pool};
use crate::bucket::FileResolver;
use crate::bucket::registry::JsonRegistry;
use anyhow::{Context, bail};
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

    pub fn publish(&self, name: &str) {
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

    fn load(&self, name: &str) -> anyhow::Result<LoadedConfig> {
        let config = self
            .configs
            .get(name)
            .with_context(|| format!("no DJ config {name:?}"))?;
        let pools = config
            .pools
            .iter()
            .map(|(pool_name, pool_config)| {
                let file = self.get_pool(pool_name, true)?;
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

    pub fn get_pool(&self, name: &str, require_playable: bool) -> anyhow::Result<Arc<Pool>> {
        let pool = self.pools.require(name)?;
        if require_playable && !pool.has_weighted_entry() {
            bail!("pool {name:?} has no entry with weight above 0");
        }
        Ok(pool)
    }
}
