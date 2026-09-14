use crate::audio::dj::config::{DJConfig, HexMessageEntry, NoisePeriodEntry, TrackEntry};
use crate::audio::dj::weighted_choice::WeightedSelector;
use std::collections::VecDeque;
use std::sync::LazyLock;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DJStateType {
    Track(usize),
    HexMessage(usize),
    Noise(usize),
}

pub struct WeightedScheduler {
    config: DJConfig,
    recent_history: VecDeque<DJStateType>,
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
            recent_history: VecDeque::with_capacity(config.recent_history_size),
            config,
            track_selector,
            hex_message_selector,
            noise_selector,
        }
    }

    pub fn update_config(&mut self, config: DJConfig) {
        self.config = config;
    }

    pub fn next_state(&mut self) -> DJStateType {
        let state = match self.choose_state_type() {
            StateCategory::Track => self.choose_track(),
            StateCategory::HexMessage => self.choose_hex_message(),
            StateCategory::Noise => self.choose_noise(),
        };
        self.add_to_history(state.clone());
        state
    }

    pub fn next_state_of_type(
        &mut self,
        filter: crate::audio::dj::manager::DJStateTypeFilter,
    ) -> DJStateType {
        let state = match filter {
            crate::audio::dj::manager::DJStateTypeFilter::Track => self.choose_track(),
            crate::audio::dj::manager::DJStateTypeFilter::HexMessage => self.choose_hex_message(),
            crate::audio::dj::manager::DJStateTypeFilter::Noise => self.choose_noise(),
        };

        self.add_to_history(state.clone());
        state
    }

    fn choose_state_type(&self) -> StateCategory {
        use rand::Rng;
        let weights = &self.config.state_weights;
        let categories = [
            (
                StateCategory::Track,
                weights.track,
                self.config.track_pool.is_empty(),
            ),
            (
                StateCategory::HexMessage,
                weights.hex_message,
                self.config.hex_messages.is_empty(),
            ),
            (
                StateCategory::Noise,
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
            return StateCategory::Noise;
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
        StateCategory::Noise
    }

    fn choose_track(&mut self) -> DJStateType {
        let track_pool = &self.config.track_pool;
        let index = self.track_selector.choose(track_pool, |entry| entry.weight);
        DJStateType::Track(index)
    }

    fn choose_hex_message(&mut self) -> DJStateType {
        let hex_messages = &self.config.hex_messages;
        let index = self
            .hex_message_selector
            .choose(hex_messages, |entry| entry.weight);
        DJStateType::HexMessage(index)
    }

    fn choose_noise(&mut self) -> DJStateType {
        let noise_periods = &self.config.noise_periods;
        let index = self
            .noise_selector
            .choose(noise_periods, |entry| entry.weight);
        DJStateType::Noise(index)
    }

    fn add_to_history(&mut self, state: DJStateType) {
        if self.recent_history.len() >= self.config.recent_history_size {
            self.recent_history.pop_front();
        }
        self.recent_history.push_back(state);
    }

    pub fn get_track(&self, index: usize) -> Option<&TrackEntry> {
        self.config.track_pool.get(index)
    }

    pub fn get_hex_message(&self, index: usize) -> Option<&HexMessageEntry> {
        self.config.hex_messages.get(index)
    }

    pub fn get_noise_period(&self, index: usize) -> &NoisePeriodEntry {
        self.config
            .noise_periods
            .get(index)
            .unwrap_or(&DEFAULT_NOISE_PERIOD)
    }

    pub fn config(&self) -> &DJConfig {
        &self.config
    }
}

enum StateCategory {
    Track,
    HexMessage,
    Noise,
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
        let mut scheduler = WeightedScheduler::new(config(Vec::new()));
        for _ in 0..100 {
            assert!(matches!(scheduler.next_state(), DJStateType::Noise(_)));
        }
    }

    #[test]
    fn empty_categories_are_skipped() {
        let mut scheduler = WeightedScheduler::new(config(vec![track()]));
        for _ in 0..100 {
            assert!(matches!(scheduler.next_state(), DJStateType::Track(_)));
        }
    }
}
