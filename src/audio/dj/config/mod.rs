use crate::audio::dj::segments::SegmentPool;
use crate::audio::dj::weighted_choice::Weighted;
use crate::bucket::registry::RegistryEntry;
use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};
use std::any::Any;

#[derive(Deserialize, Serialize, Debug, Default)]
pub struct DJConfig {
    pub name: String,
    pub pools: Vec<PoolEntry>,
    pub signal_profiles: Vec<SignalProfileEntry>,
    pub recent_history_size: usize,
    pub duplicate_penalty_multiplier: f32,
    pub channel_status: Option<String>,
}

impl DJConfig {
    pub fn pool<T: SegmentPool>(&self, name: &str) -> anyhow::Result<&T> {
        let entry = self
            .pools
            .iter()
            .find(|entry| entry.name == name)
            .with_context(|| format!("no pool named {name:?}"))?;
        let pool = &*entry.pool;
        (pool as &dyn Any)
            .downcast_ref()
            .with_context(|| format!("pool {name:?} is a {} pool", pool.typetag_name()))
    }
}

#[derive(Deserialize, Serialize, Debug)]
pub struct PoolEntry {
    pub name: String,
    pub weight: u32,
    #[serde(flatten)]
    pub pool: Box<dyn SegmentPool>,
}

impl Weighted for PoolEntry {
    fn weight(&self) -> f32 {
        if self.pool.is_empty() {
            0.0
        } else {
            self.weight as f32
        }
    }

    fn key(&self) -> &str {
        &self.name
    }
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct SignalProfileEntry {
    pub profile_name: String,
    pub weight: u32,
    pub fade_duration_seconds: f32,
    pub min_time_seconds: f32,
    pub max_time_seconds: f32,
}

impl Weighted for SignalProfileEntry {
    fn weight(&self) -> f32 {
        self.weight as f32
    }

    fn key(&self) -> &str {
        &self.profile_name
    }
}

impl RegistryEntry for DJConfig {
    const KIND: &'static str = "DJ config";

    fn name(&self) -> &str {
        &self.name
    }

    /// Rejects values that would make the DJ panic when it picks them.
    fn validate(&self) -> anyhow::Result<()> {
        for entry in &self.pools {
            entry
                .pool
                .validate()
                .with_context(|| format!("pool {:?}", entry.name))?;
        }
        for profile in &self.signal_profiles {
            let context = || format!("signal profile {:?}", profile.profile_name);
            ensure_seconds(profile.fade_duration_seconds, context)?;
            ensure_seconds(profile.min_time_seconds, context)?;
            if profile.min_time_seconds >= profile.max_time_seconds {
                bail!(
                    "{}: min_time_seconds must be less than max_time_seconds",
                    context()
                );
            }
        }
        Ok(())
    }
}

pub fn ensure_seconds(seconds: f32, context: impl Fn() -> String) -> anyhow::Result<()> {
    if !(seconds.is_finite() && seconds >= 0.0) {
        bail!(
            "{}: {seconds} is not a non-negative number of seconds",
            context()
        );
    }
    Ok(())
}
