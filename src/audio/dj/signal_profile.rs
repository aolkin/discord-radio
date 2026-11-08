use crate::audio::dj::config::DurationRange;
use crate::audio::dj::tiered::{ConfigurableTier, TimedValue};
use crate::audio::dj::weighted_choice::{Identity, Weighted, WeightedPool};
use field_trait::field_trait_impl;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;

pub const DEFAULT_PROFILE_FADE: Duration = Duration::from_secs(2);

pub fn default_profile_fade(tier: ConfigurableTier) -> Option<Duration> {
    match tier {
        ConfigurableTier::Manual => None,
        ConfigurableTier::Override => Some(DEFAULT_PROFILE_FADE),
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SignalProfilePlayback {
    pub name: String,
    pub fade_in_duration: Option<Duration>,
    pub fade_out_duration: Option<Duration>,
}

impl SignalProfilePlayback {
    pub fn fade_in(&self) -> Duration {
        self.fade_in_duration
            .unwrap_or_else(|| Duration::from_secs(1))
    }

    pub fn fade_out(&self) -> Duration {
        self.fade_out_duration
            .unwrap_or_else(|| Duration::from_secs_f32(1.5))
    }
}

impl From<String> for SignalProfilePlayback {
    fn from(name: String) -> Self {
        Self {
            name,
            fade_in_duration: None,
            fade_out_duration: None,
        }
    }
}

impl<T> From<&T> for SignalProfilePlayback
where
    T: Clone + Into<String>,
{
    fn from(value: &T) -> Self {
        let name: String = value.clone().into();
        name.into()
    }
}

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
