use crate::audio::dj::config::{DJConfig, HexMessageEntry, NoisePeriodEntry, TrackEntry};
use crate::audio::dj::manager::DJStateType;
use crate::audio::dj::weighted_choice::WeightedSelector;
use rand::Rng;
use std::sync::LazyLock;

#[derive(Clone, strum::Display)]
pub enum DJStateEntry {
    Track(TrackEntry),
    HexMessage(HexMessageEntry),
    Noise(NoisePeriodEntry),
}

pub struct WeightedScheduler {
    config: DJConfig,
    track_selector: WeightedSelector,
    hex_message_selector: WeightedSelector,
    noise_selector: WeightedSelector,
}

impl WeightedScheduler {
    pub fn new(config: DJConfig) -> Self {
        let track_selector = WeightedSelector::new(
            config.recent_history_size,
            config.duplicate_penalty_multiplier,
        );
        let hex_message_selector = WeightedSelector::new(
            config.recent_history_size,
            config.duplicate_penalty_multiplier,
        );
        let noise_selector = WeightedSelector::new(
            config.recent_history_size,
            config.duplicate_penalty_multiplier,
        );
        Self {
            config,
            track_selector,
            hex_message_selector,
            noise_selector,
        }
    }

    pub fn update_config(&mut self, config: DJConfig) {
        self.config = config;
    }

    pub fn next_state(&mut self, filter: Option<DJStateType>) -> DJStateEntry {
        match filter.unwrap_or_else(|| self.choose_state_type()) {
            DJStateType::Track => {
                let track = self.track_selector.choose(&self.config.track_pool);
                track.map(|track| DJStateEntry::Track(track.to_owned()))
            }
            DJStateType::HexMessage => {
                let message = self.hex_message_selector.choose(&self.config.hex_messages);
                message.map(|message| DJStateEntry::HexMessage(message.to_owned()))
            }
            DJStateType::Noise => {
                let noise = self.noise_selector.choose(&self.config.noise_periods);
                noise.map(|noise| DJStateEntry::Noise(noise.to_owned()))
            }
        }
        .unwrap_or_else(|| DJStateEntry::Noise(DEFAULT_NOISE_PERIOD.to_owned()))
    }

    fn choose_state_type(&self) -> DJStateType {
        let weights = &self.config.state_weights;
        let categories = [
            (
                DJStateType::Track,
                weights.track,
                self.config.track_pool.is_empty(),
            ),
            (
                DJStateType::HexMessage,
                weights.hex_message,
                self.config.hex_messages.is_empty(),
            ),
            (
                DJStateType::Noise,
                weights.noise,
                self.config.noise_periods.is_empty(),
            ),
        ];

        let total: u32 = categories
            .iter()
            .filter(|(_, _, empty)| !empty)
            .map(|(_, weight, _)| weight)
            .sum();
        if total == 0 {
            return DJStateType::Noise;
        }

        let mut rng = rand::rng();
        let roll: u32 = rng.random_range(0..total);

        let mut cumulative = 0;
        for (category, weight, empty) in categories {
            if empty {
                continue;
            }
            cumulative += weight;
            if roll < cumulative {
                return category;
            }
        }
        DJStateType::Noise
    }

    pub fn config(&self) -> &DJConfig {
        &self.config
    }
}

static DEFAULT_NOISE_PERIOD: LazyLock<NoisePeriodEntry> = LazyLock::new(|| NoisePeriodEntry {
    noise_profile: "default".to_string(),
    min_duration_seconds: 2.0,
    max_duration_seconds: 2.0,
    weight: 1,
});

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::dj::config::StateWeights;

    fn config(track_pool: Vec<TrackEntry>) -> DJConfig {
        DJConfig {
            name: "test".to_string(),
            track_pool,
            hex_messages: Vec::new(),
            hex_message_announcements: None,
            hex_message_defaults: Default::default(),
            noise_periods: Vec::new(),
            signal_profiles: Vec::new(),
            state_weights: StateWeights {
                track: 1,
                hex_message: 1,
                noise: 1,
            },
            recent_history_size: 4,
            duplicate_penalty_multiplier: 0.5,
            channel_status: None,
        }
    }

    fn track() -> TrackEntry {
        TrackEntry {
            filename: "audio/song.ogg".to_string(),
            weight: 1,
            max_duration_seconds: None,
            allow_subsection: None,
            signal_profile: None,
            volume: None,
            channel_status: None,
        }
    }

    #[test]
    fn all_pools_empty_falls_back_to_noise() {
        let scheduler = WeightedScheduler::new(config(Vec::new()));
        for _ in 0..100 {
            assert!(matches!(scheduler.choose_state_type(), DJStateType::Noise));
        }
    }

    #[test]
    fn empty_categories_are_skipped() {
        let scheduler = WeightedScheduler::new(config(vec![track()]));
        for _ in 0..100 {
            assert!(matches!(scheduler.choose_state_type(), DJStateType::Track,));
        }
    }
}
