use crate::audio::dj::config::{DJConfig, SignalProfileEntry};
use crate::audio::dj::segments::{NoisePeriodEntry, NoiseSegment, Segment};
use crate::audio::dj::weighted_choice::{WeightedSelector, choose_index};
use crate::state::Data;
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

    /// Picks a segment from a pool chosen by weight, limited to pools of
    /// `kind` when set. A forced pool with weight 0 counts as weight 1.
    pub async fn next_segment(&mut self, kind: Option<&str>, bot_state: Data) -> Box<dyn Segment> {
        let weights: Vec<f32> = self
            .config
            .pools
            .iter()
            .map(|pool| match kind {
                _ if pool.is_empty() => 0,
                None => pool.weight(),
                Some(kind) if pool.typetag_name() == kind => pool.weight().max(1),
                Some(_) => 0,
            } as f32)
            .collect();
        let picked = match choose_index(&weights) {
            Some(index) => {
                self.config.pools[index]
                    .next_segment(&mut self.pool_selectors[index], bot_state)
                    .await
            }
            None => None,
        };
        picked.unwrap_or_else(|| Box::new(NoiseSegment::from(&*DEFAULT_NOISE_PERIOD)))
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
