use crate::audio::dj::config::NoisePeriodEntry;
use crate::audio::dj::segments::signals::Completed;
use crate::audio::dj::segments::{
    CommonSegmentPlaybackConfig, Segment, SegmentPlaybackConfig, SignalProfilePlayback,
    StateDisplay, TimedSegment,
};
use async_trait::async_trait;
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::ops::Deref;
use std::time::Duration;

#[derive(Debug, Serialize, Deserialize)]
pub enum NoiseSegment {
    Timed(TimedSegment),
    Infinite(CommonSegmentPlaybackConfig),
}

impl NoiseSegment {
    pub fn new(noise_profile: String, duration: Option<Duration>) -> Self {
        let playback_config = CommonSegmentPlaybackConfig {
            signal_profile: Some(SignalProfilePlayback {
                name: noise_profile,
                fade_in_duration: duration.map(|duration| duration.div_f32(2.0)),
                fade_out_duration: None,
            }),
            channel_status: None,
        };
        match duration {
            Some(duration) => Self::Timed(TimedSegment::new(playback_config, duration)),
            None => Self::Infinite(playback_config),
        }
    }

    pub fn quiet(duration: Duration) -> Self {
        Self::Timed(TimedSegment::new(Default::default(), duration))
    }

    #[allow(dead_code)]
    pub fn idle() -> Self {
        Self::Infinite(Default::default())
    }

    fn duration(&self) -> Option<Duration> {
        match self {
            NoiseSegment::Timed(timer) => Some(timer.duration),
            NoiseSegment::Infinite(_) => None,
        }
    }
}

impl Deref for NoiseSegment {
    type Target = CommonSegmentPlaybackConfig;
    fn deref(&self) -> &Self::Target {
        match self {
            NoiseSegment::Timed(timer) => timer,
            NoiseSegment::Infinite(config) => config,
        }
    }
}

#[async_trait]
#[typetag::serde]
impl Segment for NoiseSegment {
    fn completed(&self) -> Completed {
        match self {
            NoiseSegment::Timed(timer) => timer.completed(),
            NoiseSegment::Infinite(_) => Completed::never(),
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

impl From<NoisePeriodEntry> for NoiseSegment {
    fn from(noise_entry: NoisePeriodEntry) -> Self {
        let duration_secs = if noise_entry.min_duration_seconds >= noise_entry.max_duration_seconds
        {
            noise_entry.min_duration_seconds
        } else {
            rand::rng()
                .random_range(noise_entry.min_duration_seconds..noise_entry.max_duration_seconds)
        };

        Self::new(
            noise_entry.noise_profile,
            Some(Duration::from_secs_f32(duration_secs)),
        )
    }
}
