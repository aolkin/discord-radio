use crate::audio::dj::config::{
    ConfiguredPool, DJConfig, LoadedConfig, Pool, PoolEntry, SegmentDefaults,
};
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
                    playback: pool_config.playback,
                    order: pool_config.order,
                    segment_defaults: pool_config.segment_defaults.or(&file.segment_defaults),
                    file,
                })
            })
            .collect::<anyhow::Result<_>>()
            .with_context(|| format!("DJ config {name:?}"))?;
        Ok(LoadedConfig { config, pools })
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct FileReference {
    pub kind: &'static str,
    pub name: String,
}

impl std::fmt::Display for FileReference {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {:?}", self.kind, self.name)
    }
}

impl DJConfigRegistry {
    pub fn files_referencing(&self, key: &str) -> Vec<FileReference> {
        let uses_folder = |defaults: &SegmentDefaults| {
            defaults
                .hex_config
                .as_ref()
                .and_then(|hex| hex.hex_audio_folder.as_deref())
                .and_then(|folder| folder.strip_prefix("s3://"))
                .is_some_and(|folder| key.starts_with(folder))
        };

        let pools = self.pools.list().into_iter().filter_map(|name| {
            let pool = self.pools.get(&name)?;
            let references = uses_folder(&pool.segment_defaults)
                || pool.entries.iter().any(|entry| {
                    matches!(entry, PoolEntry::Track(track)
                        if track.filename.strip_prefix("s3://") == Some(key))
                });
            references.then_some(FileReference { kind: "pool", name })
        });
        let configs = self.configs.list().into_iter().filter_map(|name| {
            let config = self.configs.get(&name)?;
            config
                .pools
                .values()
                .any(|pool| uses_folder(&pool.segment_defaults))
                .then_some(FileReference {
                    kind: "DJ config",
                    name,
                })
        });

        let mut references: Vec<_> = pools.chain(configs).collect();
        references.sort_by(|a, b| (a.kind, &a.name).cmp(&(b.kind, &b.name)));
        references
    }
}
