use crate::audio::dj::config::{
    DJConfig, HexMessageEntry, NoisePeriodEntry, SignalProfileEntry, TrackEntry,
};
use crate::audio::dj::manager::DJStateType;
use crate::audio::dj::weighted_choice::WeightedSelector;
use rand::RngExt;
use std::sync::{Arc, LazyLock};

#[derive(Clone, strum::Display)]
pub enum DJStateEntry {
    Track(TrackEntry),
    HexMessage(HexMessageEntry),
    Noise(NoisePeriodEntry),
}

pub struct WeightedScheduler {
    config: Arc<DJConfig>,
    track_selector: WeightedSelector,
    hex_message_selector: WeightedSelector,
    noise_selector: WeightedSelector,
    signal_profile_selector: WeightedSelector,
}

impl WeightedScheduler {
    pub fn new(config: Arc<DJConfig>) -> Self {
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
            signal_profile_selector: WeightedSelector::new(5, 0.3),
        }
    }

    pub fn next_signal_profile(&mut self) -> SignalProfileEntry {
        self.signal_profile_selector
            .choose(&self.config.signal_profiles)
            .unwrap_or(&DEFAULT_SIGNAL_PROFILE)
            .to_owned()
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

    pub fn config(&self) -> &Arc<DJConfig> {
        &self.config
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

#[cfg(test)]
mod tests;
