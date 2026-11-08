use crate::audio::dj::config::{DurationRange, EntryPlaybackConfig, SegmentDefaults};
use crate::audio::dj::segments::signals::Completed;
use crate::audio::dj::segments::{
    CommonSegmentPlaybackConfig, Segment, SegmentPlaybackConfig, StateDisplay,
};
use crate::audio::dj::signal_profile::SignalProfilePlayback;
use crate::audio::dj::weighted_choice::{Identity, Weighted};
use async_trait::async_trait;
use field_trait::field_trait_impl;
use serde::{Deserialize, Serialize};
use std::ops::Deref;
use std::time::{Duration, Instant};

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct NoiseEntry {
    #[serde(flatten)]
    pub duration: DurationRange,
    pub weight: u32,
    pub fade_duration_seconds: Option<f32>,
    #[serde(flatten)]
    pub playback: EntryPlaybackConfig,
}

#[field_trait_impl]
impl Weighted for NoiseEntry {
    fn identity(&self) -> Identity {
        Identity::of((
            self.duration.min_duration_seconds.to_bits(),
            self.duration.max_duration_seconds.to_bits(),
            self.fade_duration_seconds.map(f32::to_bits),
            &self.playback,
        ))
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
        duration: Option<Duration>,
        fade: Option<Duration>,
        config: CommonSegmentPlaybackConfig,
    ) -> Self {
        let fade_in_duration = fade.or_else(|| duration.map(|duration| duration.div_f32(2.0)));
        let config = CommonSegmentPlaybackConfig {
            signal_profile: config.signal_profile.map(|profile| SignalProfilePlayback {
                fade_in_duration,
                ..profile
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
            Some(noise_entry.duration.sample()),
            noise_entry
                .fade_duration_seconds
                .map(Duration::from_secs_f32),
            segment_defaults.playback_config(noise_entry.playback.clone()),
        )
    }
}
