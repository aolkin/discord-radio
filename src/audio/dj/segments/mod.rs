mod hex;
mod noise;
mod track;

pub use hex::HexSegment;
pub use noise::NoiseSegment;
pub use track::TrackSegment;

use crate::audio::dj::serde::{deser_instant, ser_instant};
use crate::audio::processing::mixer::{Mixer, TrackCommand, TrackHandle};
use crate::audio::processing::processor::AudioProcessor;
use crate::state::BotState;
use anyhow::Context;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serenity::http::Http;
use serenity::model::id::GuildId;
use std::borrow::ToOwned;
use std::fmt::Debug;
use std::ops::Deref;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{OwnedRwLockMappedWriteGuard, OwnedRwLockWriteGuard, RwLock};

pub struct SegmentCtx<'a> {
    pub guild_id: &'a GuildId,
    pub bot_state: &'a BotState,
    pub http: &'a Http,
}

impl SegmentCtx<'_> {
    pub async fn audio_processor(&self) -> anyhow::Result<Arc<RwLock<AudioProcessor>>> {
        self.bot_state
            .audio_processors
            .read()
            .await
            .get(self.guild_id)
            .cloned()
            .with_context(|| format!("No audio processor for guild {}", self.guild_id))
    }

    pub async fn mixer(
        &self,
    ) -> anyhow::Result<OwnedRwLockMappedWriteGuard<AudioProcessor, Mixer>> {
        let processor = self.audio_processor().await?.write_owned().await;
        Ok(OwnedRwLockWriteGuard::map(processor, |p| p.mixer_mut()))
    }
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

pub struct StateDisplay {
    pub name: &'static str,
    pub subject: Option<String>,
    pub elapsed: Duration,
    pub total: Option<Duration>,
}

#[async_trait]
#[typetag::serde(tag = "type")]
pub trait Segment: Send + Sync + Debug + SegmentPlaybackConfig {
    async fn is_complete(&self) -> bool;
    fn elapsed(&self) -> Duration;

    async fn restore(&self, _ctx: &SegmentCtx) -> anyhow::Result<()> {
        Ok(())
    }
    async fn enter(&self, _ctx: &SegmentCtx) -> anyhow::Result<()> {
        Ok(())
    }
    async fn exit(&self, _ctx: &SegmentCtx) {}

    fn announcement(&self) -> &Option<String> {
        &None
    }

    fn loggable_properties(&self) -> serde_json::Value {
        serde_json::json!({})
    }

    fn state_name(&self) -> &'static str;
    fn state_display(&self) -> StateDisplay;
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
    pub fn new(playback_config: CommonSegmentPlaybackConfig, duration: Duration) -> Self {
        Self {
            playback_config,
            started_at: std::time::Instant::now(),
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

#[derive(Debug, Default)]
pub struct TrackSlot(std::sync::Mutex<Option<TrackHandle>>);

impl TrackSlot {
    pub fn set(&self, handle: TrackHandle) {
        *self.0.lock().unwrap() = Some(handle);
    }

    pub fn stop(&self, fade: Duration) {
        if let Some(handle) = self.0.lock().unwrap().take() {
            let _ = handle.send(TrackCommand::Stop { fade, at: None });
        }
    }
}
