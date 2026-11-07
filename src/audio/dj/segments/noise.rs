use crate::audio::dj::config::{DurationRange, SegmentDefaults, SharedEntryFields};
use crate::audio::dj::segments::signals::Completed;
use crate::audio::dj::segments::{
    CommonSegmentPlaybackConfig, Segment, SegmentPlaybackConfig, SignalProfilePlayback,
    StateDisplay,
};
use crate::audio::dj::state_machine::DEFAULT_PROFILE_FADE;
use crate::audio::dj::tiered::TimedValue;
use crate::audio::dj::weighted_choice::{Identity, Weighted};
use async_trait::async_trait;
use field_trait::field_trait_impl;
use serde::{Deserialize, Serialize};
use std::ops::Deref;
use std::time::{Duration, Instant};

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct NoiseEntry {
    pub noise_profile: String,
    #[serde(flatten)]
    pub duration: DurationRange,
    pub weight: u32,
    pub fade_duration_seconds: Option<f32>,
    #[serde(flatten)]
    pub shared: SharedEntryFields,
}

#[field_trait_impl]
impl Weighted for NoiseEntry {
    fn identity(&self) -> Identity {
        Identity::of((
            &self.noise_profile,
            self.duration.min_duration_seconds.to_bits(),
            self.duration.max_duration_seconds.to_bits(),
            self.fade_duration_seconds.map(f32::to_bits),
            &self.shared.announcement,
        ))
    }
}

impl Default for NoiseEntry {
    fn default() -> Self {
        Self {
            noise_profile: "clear".to_string(),
            weight: 1,
            fade_duration_seconds: Some(3.0),
            duration: DurationRange::default(),
            shared: SharedEntryFields::default(),
        }
    }
}

impl From<&NoiseEntry> for TimedValue<SignalProfilePlayback> {
    fn from(entry: &NoiseEntry) -> Self {
        let fade = entry
            .fade_duration_seconds
            .map_or(DEFAULT_PROFILE_FADE, Duration::from_secs_f32);
        let profile = SignalProfilePlayback {
            fade_in_duration: Some(fade),
            ..entry.noise_profile.clone().into()
        };
        TimedValue::temporary(profile, fade + entry.duration.sample())
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub enum NoiseSegment {
    Timed {
        config: CommonSegmentPlaybackConfig,
        duration: Duration,
    },
    Infinite {
        config: CommonSegmentPlaybackConfig,
    },
}

impl NoiseSegment {
    pub fn new(
        noise_profile: String,
        duration: Option<Duration>,
        fade: Option<Duration>,
        config: CommonSegmentPlaybackConfig,
    ) -> Self {
        let config = CommonSegmentPlaybackConfig {
            signal_profile: Some(SignalProfilePlayback {
                name: noise_profile,
                fade_in_duration: fade.or_else(|| duration.map(|duration| duration.div_f32(2.0))),
                fade_out_duration: None,
            }),
            ..config
        };
        match duration {
            Some(duration) => Self::Timed { config, duration },
            None => Self::Infinite { config },
        }
    }

    pub fn quiet(duration: Duration) -> Self {
        Self::Timed {
            config: Default::default(),
            duration,
        }
    }

    pub fn idle() -> Self {
        Self::Infinite {
            config: Default::default(),
        }
    }

    fn duration(&self) -> Option<Duration> {
        match self {
            NoiseSegment::Timed { duration, .. } => Some(*duration),
            NoiseSegment::Infinite { .. } => None,
        }
    }
}

impl Deref for NoiseSegment {
    type Target = CommonSegmentPlaybackConfig;
    fn deref(&self) -> &Self::Target {
        match self {
            NoiseSegment::Timed { config, .. } | NoiseSegment::Infinite { config } => config,
        }
    }
}

#[async_trait]
#[typetag::serde]
impl Segment for NoiseSegment {
    fn completed(&self, elapsed: Duration) -> Completed {
        match self {
            NoiseSegment::Timed { duration, .. } => {
                Completed::at(Instant::now() + duration.saturating_sub(elapsed))
            }
            NoiseSegment::Infinite { .. } => Completed::never(),
        }
    }

    fn loggable_properties(&self) -> serde_json::Value {
        serde_json::json!({
            "noise_profile": self.signal_profile().map(|profile| profile.name),
            "duration_secs": self.duration().map(|duration| duration.as_secs_f32()),
        })
    }

    fn state_name(&self) -> &'static str {
        "playing_noise"
    }

    fn state_display(&self) -> StateDisplay {
        let subject = self.signal_profile().map(|profile| profile.name);
        StateDisplay {
            name: if subject.is_some() {
                "PlayingNoise"
            } else {
                "Idle"
            },
            subject,
            total: self.duration(),
        }
    }
}

impl NoiseSegment {
    pub fn from_entry(noise_entry: &NoiseEntry, segment_defaults: &SegmentDefaults) -> Self {
        Self::new(
            noise_entry.noise_profile.clone(),
            Some(noise_entry.duration.sample()),
            noise_entry
                .fade_duration_seconds
                .map(Duration::from_secs_f32),
            segment_defaults.playback_config(None, None, noise_entry.shared.clone()),
        )
    }
}
