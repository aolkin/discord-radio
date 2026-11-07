use crate::audio::dj::config::{ConfiguredPool, DJConfig, LoadedConfig, Pool};
use crate::audio::dj::manager::DEFAULT_DJ_CONFIG;
use crate::bucket::FileResolver;
use crate::bucket::registry::JsonRegistry;
use anyhow::Context;
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast::error::RecvError;
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
    ) -> &'static Self {
        let registry: &'static Self = Box::leak(Box::new(Self {
            configs: JsonRegistry::from_storage(configs_path, file_resolver.clone()).await,
            pools: JsonRegistry::from_storage(pools_path, file_resolver).await,
            loaded: Mutex::default(),
        }));
        for name in registry.configs.list() {
            registry.publish(&name);
        }
        registry.spawn_publisher();
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

    fn spawn_publisher(&'static self) {
        let mut configs = self.configs.subscribe();
        let mut pools = self.pools.subscribe();
        tokio::spawn(async move {
            loop {
                let changed = tokio::select! {
                    name = configs.recv() => name.map(|name| self.publish(&name)),
                    name = pools.recv() => name.map(|name| self.publish_pool(&name)),
                };
                match changed {
                    Ok(()) => {}
                    Err(RecvError::Lagged(_)) => {
                        for name in self.configs.list() {
                            self.publish(&name);
                        }
                    }
                    Err(RecvError::Closed) => return,
                }
            }
        });
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
        let config = match self.configs.get(name) {
            Some(config) => config,
            None if name == DEFAULT_DJ_CONFIG => Arc::new(DJConfig {
                name: name.to_string(),
                ..DJConfig::default()
            }),
            None => anyhow::bail!("no DJ config {name:?}"),
        };
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
