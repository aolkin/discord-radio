use crate::audio::dj::config::{ConfiguredPool, DJConfig, LoadedConfig, Pool};
use crate::bucket::FileResolver;
use crate::bucket::registry::JsonRegistry;
use anyhow::{Context, bail};
use std::sync::Arc;
use tokio::sync::watch;

pub struct DJConfigRegistry {
    pub configs: JsonRegistry<DJConfig>,
    pub pools: JsonRegistry<Pool>,
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
        };
        for name in registry.configs.list() {
            if let Err(e) = registry.get(&name) {
                tracing::warn!("Failed to load DJ config {name:?}: {e:#}");
            }
        }
        registry
    }

    pub fn get(&self, name: &str) -> anyhow::Result<LoadedConfig> {
        let changes = self.subscribe(name);
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
        Ok(LoadedConfig {
            config,
            pools,
            changes,
        })
    }

    pub fn subscribe(&self, name: &str) -> Vec<watch::Receiver<()>> {
        let mut changes = vec![self.configs.subscribe(name)];
        if let Some(config) = self.configs.get(name) {
            changes.extend(config.pools.keys().map(|pool| self.pools.subscribe(pool)));
        }
        changes
    }

    pub fn get_pool(&self, name: &str, require_playable: bool) -> anyhow::Result<Arc<Pool>> {
        let pool = self.pools.require(name)?;
        if require_playable && !pool.has_weighted_entry() {
            bail!("pool {name:?} has no entry with weight above 0");
        }
        Ok(pool)
    }
}
