use crate::audio::dj::serde::{deser_instant, ser_instant};
use crate::state::BotState;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serenity::http::Http;
use serenity::model::id::GuildId;
use std::borrow::ToOwned;
use std::fmt::{Debug, Display};
use std::ops::Deref;
use std::time::Duration;

pub struct SegmentCtx<'a> {
    pub guild_id: &'a GuildId,
    pub bot_state: &'a BotState,
    pub http: &'a Http,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SignalProfilePlayback {
    pub name: Option<String>,
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
            name: Some(name),
            ..Default::default()
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

pub trait SegmentPlaybackConfig {
    fn signal_profile(&self) -> SignalProfilePlayback {
        Default::default()
    }

    fn channel_status(&self) -> Option<String> {
        None
    }
}

#[async_trait]
#[typetag::serde(tag = "type")]
pub trait Segment: Send + Sync + Display + Debug + SegmentPlaybackConfig {
    async fn is_complete(&self, ctx: &SegmentCtx) -> bool;

    async fn restore(&self, _ctx: &SegmentCtx) -> anyhow::Result<()> {
        Ok(())
    }
    async fn enter(&self, _ctx: &SegmentCtx) -> anyhow::Result<()> {
        Ok(())
    }
    async fn exit(&self, _ctx: &SegmentCtx) -> anyhow::Result<()> {
        Ok(())
    }

    fn loggable_properties(&self) -> serde_json::Value {
        serde_json::json!({})
    }

    fn state_info_display(&self) -> (String, String);
    fn current_state_display(&self) -> String;
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct CommonSegmentPlaybackConfig {
    pub signal_profile: Option<SignalProfilePlayback>,
    pub channel_status: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct BasicSegmentPlaybackConfig {
    playback_config: CommonSegmentPlaybackConfig,
}

impl Deref for BasicSegmentPlaybackConfig {
    type Target = CommonSegmentPlaybackConfig;
    fn deref(&self) -> &Self::Target {
        &self.playback_config
    }
}

impl From<CommonSegmentPlaybackConfig> for BasicSegmentPlaybackConfig {
    fn from(playback: CommonSegmentPlaybackConfig) -> Self {
        Self {
            playback_config: playback,
        }
    }
}

impl<T: Deref<Target = CommonSegmentPlaybackConfig>> SegmentPlaybackConfig for T {
    fn signal_profile(&self) -> SignalProfilePlayback {
        self.deref().signal_profile.to_owned().unwrap_or_default()
    }

    fn channel_status(&self) -> Option<String> {
        self.deref().channel_status.to_owned()
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TimedSegment {
    playback_config: CommonSegmentPlaybackConfig,
    #[serde(serialize_with = "ser_instant", deserialize_with = "deser_instant")]
    started_at: std::time::Instant,
    duration: Duration,
}

impl TimedSegment {
    pub fn new(
        playback_config: CommonSegmentPlaybackConfig,
        started_at: std::time::Instant,
        duration: Duration,
    ) -> Self {
        Self {
            playback_config,
            started_at,
            duration,
        }
    }

    pub fn is_elapsed(&self) -> bool {
        self.started_at.elapsed() >= self.duration
    }
}

impl Deref for TimedSegment {
    type Target = CommonSegmentPlaybackConfig;
    fn deref(&self) -> &Self::Target {
        &self.playback_config
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct NoiseSegment {
    timed: TimedSegment,
}

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
        Self {
            timed: TimedSegment::new(playback_config, std::time::Instant::now(), duration),
        }
    }

    pub fn idle(duration: Duration) -> Self {
        Self {
            timed: TimedSegment::new(Default::default(), std::time::Instant::now(), duration),
        }
    }

    fn profile_name(&self) -> Option<String> {
        self.signal_profile().name
    }
}

impl Deref for NoiseSegment {
    type Target = CommonSegmentPlaybackConfig;
    fn deref(&self) -> &Self::Target {
        &self.timed
    }
}

impl Display for NoiseSegment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "playing_noise")
    }
}

#[async_trait]
#[typetag::serde]
impl Segment for NoiseSegment {
    async fn is_complete(&self, _ctx: &SegmentCtx) -> bool {
        self.timed.is_elapsed()
    }

    async fn exit(&self, ctx: &SegmentCtx) -> anyhow::Result<()> {
        let duration_secs = self.timed.started_at.elapsed().as_secs_f64();
        if let Some(metrics) = ctx.bot_state.metrics.read().await.as_ref() {
            metrics.record_noise_state_duration(
                ctx.guild_id.get(),
                self.profile_name().as_deref().unwrap_or_default(),
                duration_secs,
            );
        }
        Ok(())
    }

    fn loggable_properties(&self) -> serde_json::Value {
        serde_json::json!({
            "noise_profile": self.profile_name(),
            "duration_secs": self.timed.duration.as_secs_f32(),
        })
    }

    fn state_info_display(&self) -> (String, String) {
        let elapsed = self.timed.started_at.elapsed().as_secs_f32();
        let total = self.timed.duration.as_secs_f32();
        let details = match self.profile_name() {
            Some(name) => format!("Profile: {name} ({elapsed:.1}s / {total:.1}s)"),
            None => format!("No profile ({elapsed:.1}s / {total:.1}s)"),
        };
        ("PlayingNoise".into(), details)
    }

    fn current_state_display(&self) -> String {
        let elapsed = self.timed.started_at.elapsed().as_secs();
        let total = self.timed.duration.as_secs();
        match self.profile_name() {
            Some(name) => format!("Playing noise with profile: **{name}** ({elapsed}/{total}s)"),
            None => format!("Playing noise with no profile ({elapsed}/{total}s)"),
        }
    }
}
