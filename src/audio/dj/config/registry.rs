use crate::audio::dj::config::{ConfiguredPool, DJConfig, Pool};
use crate::bucket::FileResolver;
use crate::bucket::registry::JsonRegistry;
use anyhow::{Context, bail};
use std::sync::Arc;

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

    pub fn get(&self, name: &str) -> anyhow::Result<(Arc<DJConfig>, Arc<[ConfiguredPool]>)> {
        let config = self
            .configs
            .get(name)
            .with_context(|| format!("no DJ config {name:?}"))?;
        let pools = config
            .pools
            .iter()
            .map(|(pool_name, pool_config)| {
                let file = self.pools.require(pool_name)?;
                if !file.has_weighted_entry() {
                    bail!("pool {pool_name:?} has no entry of weight above 0");
                }
                Ok(ConfiguredPool {
                    name: pool_name.clone(),
                    weight: pool_config.weight.get(),
                    segment_defaults: pool_config.segment_defaults.or(&file.segment_defaults),
                    file,
                })
            })
            .collect::<anyhow::Result<_>>()
            .with_context(|| format!("DJ config {name:?}"))?;
        Ok((config, pools))
    }
}
