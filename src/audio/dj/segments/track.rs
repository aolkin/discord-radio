use super::{
    CommonSegmentPlaybackConfig, Segment, SegmentCtx, SegmentPlaybackConfig, StateDisplay,
    TimedSegment, TrackSlot,
};
use crate::audio::dj::config::TrackEntry;
use crate::audio::duration::DurationCache;
use crate::audio::mixer::{MixerTrackArgs, TrackCommand};
use crate::audio::tracks::open_track_source;
use async_trait::async_trait;
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::ops::Deref;
use std::sync::Mutex;
use std::time::Duration;
use tokio::sync::oneshot::{self, error::TryRecvError};

const FADE_TIME: Duration = Duration::from_secs(1);

#[derive(Debug, Serialize, Deserialize)]
pub struct TrackSegment {
    timer: TimedSegment,
    filename: String,
    volume: f32,
    start_position: Option<Duration>,
    #[serde(skip)]
    track: TrackSlot,
    #[serde(skip)]
    end_trigger: Mutex<Option<oneshot::Receiver<()>>>,
}

impl Deref for TrackSegment {
    type Target = CommonSegmentPlaybackConfig;
    fn deref(&self) -> &Self::Target {
        &self.timer
    }
}

impl TrackSegment {
    pub fn new(
        filename: String,
        volume: f32,
        start_position: Option<Duration>,
        duration: Duration,
        signal_profile: Option<String>,
        channel_status: Option<&str>,
    ) -> Self {
        let playback_config = CommonSegmentPlaybackConfig {
            signal_profile: signal_profile.map(Into::into),
            channel_status: channel_status.map(with_status_emoji),
        };
        Self {
            timer: TimedSegment::new(playback_config, duration),
            filename,
            volume,
            start_position,
            track: Default::default(),
            end_trigger: Default::default(),
        }
    }

    async fn start_at(
        &self,
        ctx: &SegmentCtx<'_>,
        start_position: Option<Duration>,
        play_duration: Duration,
    ) -> anyhow::Result<()> {
        let path = ctx.bot_state.file_resolver.resolve(&self.filename).await?;
        let source = open_track_source(&path, start_position).map_err(|e| anyhow::anyhow!(e))?;
        let track = ctx
            .audio_processor()
            .await?
            .write()
            .await
            .mixer_mut()
            .add_track(
                Box::new(source),
                MixerTrackArgs {
                    volume: self.volume,
                    fade_in: FADE_TIME,
                    ..Default::default()
                },
            );
        let (add_trigger, end_trigger) = TrackCommand::add_trigger(play_duration);
        anyhow::ensure!(track.send(add_trigger)?, "Track {:?} has ended", track.id);
        let stop = TrackCommand::Stop {
            fade: FADE_TIME,
            at: Some(play_duration),
        };
        anyhow::ensure!(track.send(stop)?, "Track {:?} has ended", track.id);
        self.track.set(track);
        *self.end_trigger.lock().unwrap() = Some(end_trigger);
        Ok(())
    }

    pub async fn from_entry(track_entry: TrackEntry, duration_cache: &DurationCache) -> Self {
        let duration = duration_cache.get_duration(&track_entry.filename).await;
        let (start_position, play_duration) = if track_entry.allow_subsection.unwrap_or(false) {
            if let Some(total_duration) = duration {
                if total_duration.as_secs_f32() > 240.0 {
                    let mut rng = rand::rng();
                    let subsection_duration =
                        Duration::from_secs_f32(rng.random_range(120.0..240.0));
                    let latest_start =
                        total_duration.as_secs_f32() - subsection_duration.as_secs_f32();
                    let start_secs = rng.random_range(0.0..latest_start.max(0.0));
                    (
                        Some(Duration::from_secs_f32(start_secs)),
                        subsection_duration,
                    )
                } else {
                    (None, duration.unwrap_or(Duration::from_secs(60)))
                }
            } else {
                (None, Duration::from_secs(60))
            }
        } else {
            let max_dur = track_entry
                .max_duration_seconds
                .map(Duration::from_secs_f32)
                .or(duration)
                .unwrap_or(Duration::from_secs(60));
            (None, max_dur)
        };

        let volume = track_entry.volume.unwrap_or(1.0);

        TrackSegment::new(
            track_entry.filename,
            volume,
            start_position,
            play_duration,
            track_entry.signal_profile,
            track_entry.channel_status.as_deref(),
        )
    }
}

#[async_trait]
#[typetag::serde]
impl Segment for TrackSegment {
    async fn is_complete(&self) -> bool {
        self.end_trigger
            .lock()
            .unwrap()
            .as_mut()
            .is_none_or(|rx| !matches!(rx.try_recv(), Err(TryRecvError::Empty)))
    }

    fn elapsed(&self) -> Duration {
        self.timer.started_at.elapsed()
    }

    async fn restore(&self, ctx: &SegmentCtx) -> anyhow::Result<()> {
        let elapsed = self.elapsed();
        if self.timer.is_elapsed() {
            tracing::info!(
                "DJ track '{}' has already finished (elapsed: {:.1}s, duration: {:.1}s), will advance to next state",
                self.filename,
                elapsed.as_secs_f32(),
                self.timer.duration.as_secs_f32()
            );
            return Ok(());
        }
        let position = self.start_position.unwrap_or_default() + elapsed;
        self.start_at(
            ctx,
            Some(position),
            self.timer.duration.saturating_sub(elapsed),
        )
        .await?;
        tracing::info!(
            "Restored DJ track '{}' at {:.1}s into the segment in guild {}",
            self.filename,
            elapsed.as_secs_f32(),
            ctx.guild_id
        );
        Ok(())
    }

    async fn enter(&self, ctx: &SegmentCtx) -> anyhow::Result<()> {
        self.start_at(ctx, self.start_position, self.timer.duration)
            .await?;
        tracing::info!(
            "DJ starting track '{}' (duration: {:.1}s) in guild {}",
            self.filename,
            self.timer.duration.as_secs_f32(),
            ctx.guild_id
        );
        Ok(())
    }

    async fn exit(&self, _ctx: &SegmentCtx) -> anyhow::Result<()> {
        if !self.is_complete().await {
            self.track.stop(FADE_TIME);
        }
        Ok(())
    }

    fn loggable_properties(&self) -> serde_json::Value {
        serde_json::json!({
            "filename": self.filename,
            "duration_secs": self.timer.duration.as_secs_f32(),
            "forced_profile": self.signal_profile().name,
        })
    }

    fn state_name(&self) -> &'static str {
        "playing_track"
    }

    fn state_display(&self) -> StateDisplay {
        StateDisplay {
            name: "PlayingTrack",
            subject: Some(self.filename.clone()),
            elapsed: self.elapsed(),
            total: Some(self.timer.duration),
        }
    }
}

fn with_status_emoji(status: &str) -> String {
    let starts_with_emoji = status.chars().next().is_some_and(|c| {
        matches!(c as u32,
            0x1F300..=0x1F9FF | // Misc Symbols and Pictographs, Emoticons, Transport and Map, Supplemental Symbols
            0x2600..=0x26FF |   // Misc symbols
            0x2700..=0x27BF |   // Dingbats
            0xFE00..=0xFE0F |   // Variation Selectors
            0x1F000..=0x1F02F | // Mahjong Tiles, Domino Tiles
            0x1F0A0..=0x1F0FF | // Playing Cards
            0x1F100..=0x1F64F   // Enclosed characters, Emoticons
        )
    });
    if starts_with_emoji {
        status.to_string()
    } else {
        format!("🔊 {}", status)
    }
}
