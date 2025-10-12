use crate::audio::dj::config::{DJConfig, HexMessageEntry, NoisePeriodEntry, TrackEntry};
use crate::audio::dj::weighted_choice::WeightedSelector;
use std::collections::VecDeque;

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

    pub fn next_state(&mut self) -> DJStateType {
        let state_type = self.choose_state_type();
        let state = match state_type {
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
        let total: u32 = weights.track + weights.hex_message + weights.noise;

        let mut rng = rand::rng();
        let roll: u32 = rng.random_range(0..total);

        let mut cumulative = 0;
        if roll < (cumulative + weights.track) {
            return StateCategory::Track;
        }
        cumulative += weights.track;

        if roll < (cumulative + weights.hex_message) {
            return StateCategory::HexMessage;
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

    pub fn get_noise_period(&self, index: usize) -> Option<&NoisePeriodEntry> {
        self.config.noise_periods.get(index)
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
