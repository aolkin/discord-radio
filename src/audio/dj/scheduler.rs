use crate::audio::dj::config::{
    DJConfig, HexMessageEntry, NoisePeriodEntry, SignalProfileEntry, TrackEntry,
};
use rand::Rng;
use std::collections::VecDeque;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DJStateType {
    Track(usize),
    HexMessage(usize),
    Noise(usize),
    ProfileChange(usize),
}

pub struct WeightedScheduler {
    config: DJConfig,
    recent_history: VecDeque<DJStateType>,
}

impl WeightedScheduler {
    pub fn new(config: DJConfig) -> Self {
        Self {
            recent_history: VecDeque::with_capacity(config.recent_history_size),
            config,
        }
    }

    pub fn next_state(&mut self) -> DJStateType {
        let state_type = self.choose_state_type();
        let state = match state_type {
            StateCategory::Track => self.choose_track(),
            StateCategory::HexMessage => self.choose_hex_message(),
            StateCategory::Noise => self.choose_noise(),
            StateCategory::ProfileChange => self.choose_profile(),
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
        let weights = &self.config.state_weights;
        let total: u32 =
            weights.track + weights.hex_message + weights.noise + weights.profile_change;

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
        cumulative += weights.hex_message;

        if roll < (cumulative + weights.noise) {
            return StateCategory::Noise;
        }

        StateCategory::ProfileChange
    }

    fn choose_track(&self) -> DJStateType {
        let index = self.weighted_choice(&self.config.track_pool, |entry| entry.weight);
        DJStateType::Track(index)
    }

    fn choose_hex_message(&self) -> DJStateType {
        let index = self.weighted_choice(&self.config.hex_messages, |entry| entry.weight);
        DJStateType::HexMessage(index)
    }

    fn choose_noise(&self) -> DJStateType {
        let index = self.weighted_choice(&self.config.noise_periods, |entry| entry.weight);
        DJStateType::Noise(index)
    }

    fn choose_profile(&self) -> DJStateType {
        let index = self.weighted_choice(&self.config.signal_profiles, |entry| entry.weight);
        DJStateType::ProfileChange(index)
    }

    fn weighted_choice<T, F>(&self, items: &[T], weight_fn: F) -> usize
    where
        F: Fn(&T) -> u32,
    {
        let effective_weights: Vec<f32> = items
            .iter()
            .enumerate()
            .map(|(idx, item)| {
                let base_weight = weight_fn(item) as f32;
                let penalty = self.get_penalty_for_index(idx);
                base_weight * penalty
            })
            .collect();

        let total: f32 = effective_weights.iter().sum();
        if total == 0.0 {
            return 0;
        }

        let mut rng = rand::rng();
        let roll: f32 = rng.random::<f32>() * total;

        let mut cumulative = 0.0;
        for (idx, weight) in effective_weights.iter().enumerate() {
            cumulative += weight;
            if roll < cumulative {
                return idx;
            }
        }

        items.len().saturating_sub(1)
    }

    fn get_penalty_for_index(&self, idx: usize) -> f32 {
        for (history_idx, state) in self.recent_history.iter().enumerate() {
            let state_idx = match state {
                DJStateType::Track(i) => *i,
                DJStateType::HexMessage(i) => *i,
                DJStateType::Noise(i) => *i,
                DJStateType::ProfileChange(i) => *i,
            };

            if state_idx == idx {
                let recency_factor = 1.0 - (history_idx as f32 / self.recent_history.len() as f32);
                return self.config.duplicate_penalty_multiplier * recency_factor
                    + (1.0 - self.config.duplicate_penalty_multiplier);
            }
        }

        1.0
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

    pub fn get_profile(&self, index: usize) -> Option<&SignalProfileEntry> {
        self.config.signal_profiles.get(index)
    }

    pub fn config(&self) -> &DJConfig {
        &self.config
    }
}

enum StateCategory {
    Track,
    HexMessage,
    Noise,
    ProfileChange,
}
