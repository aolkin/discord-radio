use crate::audio::dj::serde::{deser_instant, ser_instant};
use crate::state::BotState;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serenity::http::Http;
use serenity::model::id::GuildId;
use std::fmt::{Debug, Display};
use std::ops::Deref;
use std::time::Duration;

pub struct SegmentCtx<'a> {
    pub guild_id: &'a GuildId,
    pub bot_state: &'a BotState,
    pub http: &'a Http,
}

pub trait SegmentPlaybackConfig {
    fn noise_profile(&self) -> Option<String> {
        None
    }

    fn voice_channel_status(&self) -> Option<String> {
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
    pub noise_profile: Option<String>,
    pub voice_channel_status: Option<String>,
}

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
    fn noise_profile(&self) -> Option<String> {
        self.deref().noise_profile.to_owned()
    }

    fn voice_channel_status(&self) -> Option<String> {
        self.deref().voice_channel_status.to_owned()
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TimedSegment {
    playback_config: CommonSegmentPlaybackConfig,
    #[serde(serialize_with = "ser_instant", deserialize_with = "deser_instant")]
    started_at: std::time::Instant,
    duration: Duration,
}

impl Deref for TimedSegment {
    type Target = CommonSegmentPlaybackConfig;
    fn deref(&self) -> &Self::Target {
        &self.playback_config
    }
}
