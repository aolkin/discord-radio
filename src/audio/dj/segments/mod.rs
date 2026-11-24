mod noise;
mod signals;
mod track;
mod tts;

pub use noise::{NoiseEntry, NoiseSegment};
pub use track::{TrackEntry, TrackSegment};
pub use tts::{TtsMessageDefaults, TtsMessageEntry, TtsSegment, ensure_loops};

use crate::audio::dj::announcement::Announcement;
use crate::audio::dj::signal_preset::SignalPresetPlayback;
use crate::audio::processing::mixer::{TrackCommand, TrackHandle};
use crate::audio::processing::processor::ProcessorRef;
use crate::bucket::FileResolver;
use crate::state::Data;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serenity::model::id::GuildId;
pub use signals::Completed;
use std::fmt::Debug;
use std::ops::Deref;
use std::sync::Mutex;
use std::time::Duration;

pub struct SegmentCtx<'a> {
    pub guild_id: &'a GuildId,
    bot_state: Data,
    audio_processor: &'a ProcessorRef,
}

impl<'a> SegmentCtx<'a> {
    pub fn new(guild_id: &'a GuildId, bot_state: Data, audio_processor: &'a ProcessorRef) -> Self {
        Self {
            guild_id,
            bot_state,
            audio_processor,
        }
    }

    pub fn audio_processor(&self) -> &ProcessorRef {
        self.audio_processor
    }

    pub fn file_resolver(&self) -> &FileResolver {
        &self.bot_state.file_resolver
    }
}

pub trait SegmentPlaybackConfig {
    fn signal_preset(&self) -> Option<SignalPresetPlayback> {
        None
    }

    fn channel_status(&self) -> Option<String> {
        None
    }

    fn announcement(&self) -> Option<Announcement> {
        None
    }
}

pub struct StateDisplay {
    pub subject: Option<String>,
    pub total: Option<Duration>,
}

#[async_trait]
#[typetag::serde(tag = "type")]
pub trait Segment: Send + Sync + Debug + SegmentPlaybackConfig {
    fn completed(&self, elapsed: Duration) -> Completed;

    fn changed(&self) -> Completed {
        Completed::never()
    }

    async fn restore(&self, _ctx: &SegmentCtx, _elapsed: Duration) -> anyhow::Result<()> {
        Ok(())
    }
    async fn enter(&self, _ctx: &SegmentCtx) -> anyhow::Result<()> {
        Ok(())
    }
    async fn exit(&self, _ctx: &SegmentCtx) {}

    fn loggable_properties(&self) -> serde_json::Value {
        serde_json::json!({})
    }

    fn state_display(&self) -> StateDisplay;
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct CommonSegmentPlaybackConfig {
    pub signal_preset: Option<SignalPresetPlayback>,
    pub channel_status: Option<String>,
    pub announcement: Option<Announcement>,
}

impl<T: Deref<Target = CommonSegmentPlaybackConfig>> SegmentPlaybackConfig for T {
    fn signal_preset(&self) -> Option<SignalPresetPlayback> {
        self.deref().signal_preset.to_owned()
    }

    fn channel_status(&self) -> Option<String> {
        self.deref().channel_status.to_owned()
    }

    fn announcement(&self) -> Option<Announcement> {
        self.deref().announcement.to_owned()
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
