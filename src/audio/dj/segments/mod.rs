mod hex;
mod noise;
mod signals;
mod track;

pub use hex::{HexEntries, HexMessageDefaults, HexSegment};
pub use noise::{NoisePeriodEntry, NoiseSegment};
pub use track::TrackSegment;

use crate::audio::dj::weighted_choice::WeightedSelector;
use crate::audio::processing::mixer::{TrackCommand, TrackHandle};
use crate::audio::processing::processor::ProcessorRef;
use crate::bucket::FileResolver;
use crate::state::Data;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serenity::model::id::GuildId;
pub use signals::Completed;
use std::any::Any;
use std::fmt::Debug;
use std::ops::Deref;
use std::sync::Mutex;
use std::time::Duration;

pub struct SegmentCtx<'a> {
    pub guild_id: &'a GuildId,
    bot_state: Data,
}

impl<'a> SegmentCtx<'a> {
    pub fn new(guild_id: &'a GuildId, bot_state: Data) -> Self {
        Self {
            guild_id,
            bot_state,
        }
    }

    pub async fn audio_processor(&self) -> ProcessorRef {
        self.bot_state.audio_processor(*self.guild_id).await
    }

    pub fn file_resolver(&self) -> &FileResolver {
        &self.bot_state.file_resolver
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

pub trait SegmentPlaybackConfig {
    fn signal_profile(&self) -> Option<SignalProfilePlayback> {
        None
    }

    fn channel_status(&self) -> Option<String> {
        None
    }
}

pub struct StateDisplay {
    pub name: &'static str,
    pub subject: Option<String>,
    pub total: Option<Duration>,
}

#[async_trait]
#[typetag::serde(tag = "type")]
pub trait Segment: Send + Sync + Debug + SegmentPlaybackConfig {
    fn completed(&self, elapsed: Duration) -> Completed;

    async fn restore(&self, _ctx: &SegmentCtx, _elapsed: Duration) -> anyhow::Result<()> {
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

/// The entries of a pool; the DJ builds segments of one kind from them.
#[async_trait]
#[typetag::serde(tag = "kind")]
pub trait PoolEntries: Any + Send + Sync + Debug {
    fn is_empty(&self) -> bool;
    fn validate(&self) -> anyhow::Result<()>;
    async fn next_segment(
        &self,
        history: &mut WeightedSelector,
        bot_state: Data,
    ) -> Option<Box<dyn Segment>>;
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
    fn signal_profile(&self) -> Option<SignalProfilePlayback> {
        self.deref().signal_profile.to_owned()
    }

    fn channel_status(&self) -> Option<String> {
        self.deref().channel_status.to_owned()
    }
}

#[derive(Debug, Default)]
pub struct TrackSlot(Mutex<Option<TrackHandle>>);

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
