mod hex;
mod noise;
mod sequence;
mod signals;
mod track;

pub use hex::{HexMessageDefaults, HexMessageEntry, HexSegment, ensure_audio_folder, ensure_loops};
pub use noise::{NoiseEntry, NoiseSegment};
pub use sequence::{SequenceEntry, SequenceSegment};
pub use track::{TrackEntry, TrackSegment};

use crate::audio::dj::announcement::Announcement;
use crate::audio::dj::signal_profile::SignalProfilePlayback;
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

    pub fn default_hex_audio_folder(&self) -> &str {
        &self.bot_state.hex_audio_folder
    }
}

pub trait SegmentPlaybackConfig {
    fn signal_profile(&self) -> Option<SignalProfilePlayback> {
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
    pub name: &'static str,
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

    fn state_name(&self) -> &'static str;
    fn state_display(&self) -> StateDisplay;
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct CommonSegmentPlaybackConfig {
    pub signal_profile: Option<SignalProfilePlayback>,
    pub channel_status: Option<String>,
    pub announcement: Option<Announcement>,
}

impl<T: Deref<Target = CommonSegmentPlaybackConfig>> SegmentPlaybackConfig for T {
    fn signal_profile(&self) -> Option<SignalProfilePlayback> {
        self.deref().signal_profile.to_owned()
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
