use crate::audio::dj::config::DurationRange;
use crate::audio::dj::signal_preset::{DEFAULT_PRESET_FADE, SignalPresetPlayback};
use crate::audio::dj::tiered::TimedValue;
use crate::audio::dj::weighted_choice::{Identity, Weighted, WeightedPool};
use field_trait::field_trait_impl;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;

#[field_trait_impl(WeightedPool<SignalPresetEntry>)]
#[derive(Deserialize, Serialize, Debug, Default)]
pub struct SignalPresets {
    pub entries: Arc<[SignalPresetEntry]>,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct SignalPresetEntry {
    pub signal_preset: String,
    #[serde(flatten)]
    pub duration: DurationRange,
    pub weight: u32,
    pub fade_duration_seconds: Option<f32>,
}

#[field_trait_impl]
impl Weighted for SignalPresetEntry {
    fn identity(&self) -> Identity {
        Identity::of((
            &self.signal_preset,
            self.duration.min_duration_seconds.to_bits(),
            self.duration.max_duration_seconds.to_bits(),
            self.fade_duration_seconds.map(f32::to_bits),
        ))
    }
}

impl Default for SignalPresetEntry {
    fn default() -> Self {
        Self {
            signal_preset: "clear_signal".to_string(),
            weight: 1,
            fade_duration_seconds: Some(3.0),
            duration: DurationRange::default(),
        }
    }
}

impl From<&SignalPresetEntry> for TimedValue<SignalPresetPlayback> {
    fn from(entry: &SignalPresetEntry) -> Self {
        let fade = entry
            .fade_duration_seconds
            .map_or(DEFAULT_PRESET_FADE, Duration::from_secs_f32);
        let preset = SignalPresetPlayback {
            fade_in_duration: Some(fade),
            ..entry.signal_preset.clone().into()
        };
        TimedValue::temporary(preset, fade + entry.duration.sample())
    }
}
