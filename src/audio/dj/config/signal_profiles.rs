use crate::audio::dj::config::DurationRange;
use crate::audio::dj::signal_profile::{DEFAULT_PROFILE_FADE, SignalProfilePlayback};
use crate::audio::dj::tiered::TimedValue;
use crate::audio::dj::weighted_choice::{Identity, Weighted, WeightedPool};
use field_trait::field_trait_impl;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;

#[field_trait_impl(WeightedPool<SignalProfileEntry>)]
#[derive(Deserialize, Serialize, Debug, Default)]
pub struct SignalProfiles {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub history_penalty_multiplier: Option<f32>,
    pub entries: Arc<[SignalProfileEntry]>,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct SignalProfileEntry {
    pub signal_profile: String,
    #[serde(flatten)]
    pub duration: DurationRange,
    pub weight: u32,
    pub fade_duration_seconds: Option<f32>,
}

#[field_trait_impl]
impl Weighted for SignalProfileEntry {
    fn identity(&self) -> Identity {
        Identity::of((
            &self.signal_profile,
            self.duration.min_duration_seconds.to_bits(),
            self.duration.max_duration_seconds.to_bits(),
            self.fade_duration_seconds.map(f32::to_bits),
        ))
    }
}

impl Default for SignalProfileEntry {
    fn default() -> Self {
        Self {
            signal_profile: "clear".to_string(),
            weight: 1,
            fade_duration_seconds: Some(3.0),
            duration: DurationRange::default(),
        }
    }
}

impl From<&SignalProfileEntry> for TimedValue<SignalProfilePlayback> {
    fn from(entry: &SignalProfileEntry) -> Self {
        let fade = entry
            .fade_duration_seconds
            .map_or(DEFAULT_PROFILE_FADE, Duration::from_secs_f32);
        let profile = SignalProfilePlayback {
            fade_in_duration: Some(fade),
            ..entry.signal_profile.clone().into()
        };
        TimedValue::temporary(profile, fade + entry.duration.sample())
    }
}
