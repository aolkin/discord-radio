use crate::audio::dj::config::{DJConfig, SignalProfileEntry};
use crate::audio::dj::segments::{NoisePeriodEntry, NoiseSegment, Segment};
use crate::audio::dj::weighted_choice::{WeightedSelector, choose_index};
use crate::state::Data;
use anyhow::Context;
use std::sync::{Arc, LazyLock};

pub struct WeightedScheduler {
    config: Arc<DJConfig>,
    pool_selectors: Vec<WeightedSelector>,
    signal_profile_selector: WeightedSelector,
}

impl WeightedScheduler {
    pub fn new(config: Arc<DJConfig>) -> Self {
        let pool_selectors = config
            .pools
            .iter()
            .map(|_| {
                WeightedSelector::new(
                    config.recent_history_size,
                    config.duplicate_penalty_multiplier,
                )
            })
            .collect();
        Self {
            config,
            pool_selectors,
            signal_profile_selector: WeightedSelector::new(5, 0.3),
        }
    }

    pub fn next_signal_profile(&mut self) -> SignalProfileEntry {
        self.signal_profile_selector
            .choose(&self.config.signal_profiles)
            .unwrap_or(&DEFAULT_SIGNAL_PROFILE)
            .to_owned()
    }

    /// Picks a segment from the pool named `pool_name`, or else from a pool
    /// chosen by weight.
    pub async fn next_segment(
        &mut self,
        pool_name: Option<&str>,
        bot_state: Data,
    ) -> anyhow::Result<Box<dyn Segment>> {
        let pools: Vec<_> = self.config.pools.iter().collect();
        let index = match pool_name {
            Some(name) => Some(
                pools
                    .iter()
                    .position(|(key, _)| *key == name)
                    .with_context(|| format!("no pool named {name:?}"))?,
            ),
            None => {
                let weights: Vec<f32> = pools
                    .iter()
                    .map(|(_, pool)| if pool.is_empty() { 0 } else { pool.weight() } as f32)
                    .collect();
                choose_index(&weights)
            }
        };
        match index {
            Some(index) => {
                let (name, pool) = pools[index];
                pool.next_segment(&mut self.pool_selectors[index], bot_state)
                    .await
                    .with_context(|| format!("pool {name:?} is empty"))
            }
            None => Ok(Box::new(NoiseSegment::from(&*DEFAULT_NOISE_PERIOD))),
        }
    }
}

static DEFAULT_NOISE_PERIOD: LazyLock<NoisePeriodEntry> = LazyLock::new(|| NoisePeriodEntry {
    noise_profile: "default".to_string(),
    min_duration_seconds: 2.0,
    max_duration_seconds: 2.0,
    weight: 1,
});

static DEFAULT_SIGNAL_PROFILE: LazyLock<SignalProfileEntry> =
    LazyLock::new(|| SignalProfileEntry {
        profile_name: "clear".to_string(),
        weight: 1,
        fade_duration_seconds: 3.0,
        min_time_seconds: 60.0,
        max_time_seconds: 120.0,
    });
