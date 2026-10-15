use crate::audio::dj::config::NoisePeriodEntry;
use crate::audio::dj::segments::{
    CommonSegmentPlaybackConfig, Completed, Segment, SegmentPlaybackConfig, SignalProfilePlayback,
    StateDisplay, TimedSegment,
};
use async_trait::async_trait;
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::ops::Deref;
use std::time::Duration;

#[derive(Debug, Serialize, Deserialize)]
pub struct NoiseSegment(TimedSegment);

impl NoiseSegment {
    pub fn new(noise_profile: String, duration: Duration) -> Self {
        let playback_config = CommonSegmentPlaybackConfig {
            signal_profile: Some(SignalProfilePlayback {
                name: Some(noise_profile),
                fade_in_duration: Some(duration.div_f32(2.0)),
                ..Default::default()
            }),
            channel_status: None,
        };
        Self(TimedSegment::new(playback_config, duration))
    }

    pub fn idle(duration: Duration) -> Self {
        Self(TimedSegment::new(Default::default(), duration))
    }
}

impl Deref for NoiseSegment {
    type Target = CommonSegmentPlaybackConfig;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[async_trait]
#[typetag::serde]
impl Segment for NoiseSegment {
    fn completed(&self) -> Completed {
        self.0.completed()
    }

    fn elapsed(&self) -> Duration {
        self.0.started_at.elapsed()
    }

    fn loggable_properties(&self) -> serde_json::Value {
        serde_json::json!({
            "noise_profile": self.signal_profile().name,
            "duration_secs": self.0.duration.as_secs_f32(),
        })
    }

    fn state_name(&self) -> &'static str {
        "playing_noise"
    }

    fn state_display(&self) -> StateDisplay {
        let subject = self.signal_profile().name;
        StateDisplay {
            name: if subject.is_some() {
                "PlayingNoise"
            } else {
                "Idle"
            },
            subject,
            elapsed: self.0.started_at.elapsed(),
            total: Some(self.0.duration),
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
            Duration::from_secs_f32(duration_secs),
        )
    }
}
