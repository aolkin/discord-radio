use crate::audio::dj::config::ensure_seconds;
use crate::audio::dj::segments::signals::Completed;
use crate::audio::dj::segments::{
    CommonSegmentPlaybackConfig, Segment, SegmentPlaybackConfig, SegmentPool,
    SignalProfilePlayback, StateDisplay,
};
use crate::audio::dj::weighted_choice::{Weighted, WeightedSelector};
use crate::state::Data;
use async_trait::async_trait;
use rand::RngExt;
use serde::{Deserialize, Serialize};
use std::ops::Deref;
use std::time::{Duration, Instant};

#[derive(Debug, Serialize, Deserialize)]
pub struct NoisePool {
    pub entries: Vec<NoisePeriodEntry>,
}

#[derive(Deserialize, Serialize, Debug)]
pub struct NoisePeriodEntry {
    pub noise_profile: String,
    pub min_duration_seconds: f32,
    pub max_duration_seconds: f32,
    pub weight: u32,
}

impl Weighted for NoisePeriodEntry {
    fn weight(&self) -> f32 {
        self.weight as f32
    }

    fn key(&self) -> &str {
        &self.noise_profile
    }
}

#[async_trait]
#[typetag::serde(name = "noise")]
impl SegmentPool for NoisePool {
    fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn validate(&self) -> anyhow::Result<()> {
        for noise in &self.entries {
            let context = || format!("noise period {:?}", noise.noise_profile);
            ensure_seconds(noise.min_duration_seconds, context)?;
            ensure_seconds(noise.max_duration_seconds, context)?;
        }
        Ok(())
    }

    async fn next_segment(
        &self,
        history: &mut WeightedSelector,
        _bot_state: Data,
    ) -> Option<Box<dyn Segment>> {
        let entry = history.choose(&self.entries)?;
        Some(Box::new(NoiseSegment::from(entry)))
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
    pub fn new(noise_profile: String, duration: Option<Duration>) -> Self {
        let config = CommonSegmentPlaybackConfig {
            signal_profile: Some(SignalProfilePlayback {
                name: noise_profile,
                fade_in_duration: duration.map(|duration| duration.div_f32(2.0)),
                fade_out_duration: None,
            }),
            channel_status: None,
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

impl From<&NoisePeriodEntry> for NoiseSegment {
    fn from(noise_entry: &NoisePeriodEntry) -> Self {
        let duration_secs = if noise_entry.min_duration_seconds >= noise_entry.max_duration_seconds
        {
            noise_entry.min_duration_seconds
        } else {
            rand::rng()
                .random_range(noise_entry.min_duration_seconds..noise_entry.max_duration_seconds)
        };

        Self::new(
            noise_entry.noise_profile.clone(),
            Some(Duration::from_secs_f32(duration_secs)),
        )
    }
}
