use super::{
    CommonSegmentPlaybackConfig, Segment, SegmentCtx, SegmentPlaybackConfig, StateDisplay,
    TrackSlot,
};
use crate::audio::dj::config::TrackEntry;
use crate::audio::dj::segments::signals::{Completed, CompletedSignal};
use crate::audio::dj::serde::{deser_instant, ser_instant};
use crate::audio::duration::DurationCache;
use crate::audio::processing::mixer::TrackCommand;
use crate::audio::tracks::open_track_source;
use async_trait::async_trait;
use rand::RngExt;
use serde::{Deserialize, Serialize};
use std::ops::Deref;
use std::time::{Duration, Instant};

const FADE_TIME: Duration = Duration::from_secs(1);

#[derive(Debug, Serialize, Deserialize)]
pub struct TrackSegment {
    config: CommonSegmentPlaybackConfig,
    #[serde(serialize_with = "ser_instant", deserialize_with = "deser_instant")]
    started_at: Instant,
    duration: Duration,
    filename: String,
    volume: f32,
    start_position: Option<Duration>,
    #[serde(skip)]
    track: TrackSlot,
    #[serde(skip)]
    done: CompletedSignal,
}

impl Deref for TrackSegment {
    type Target = CommonSegmentPlaybackConfig;
    fn deref(&self) -> &Self::Target {
        &self.config
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
        let config = CommonSegmentPlaybackConfig {
            signal_profile: signal_profile.map(Into::into),
            channel_status: channel_status.map(with_status_emoji),
        };
        Self {
            config,
            started_at: Instant::now(),
            duration,
            filename,
            volume,
            start_position,
            track: Default::default(),
            done: Default::default(),
        }
    }

    async fn start_at(
        &self,
        ctx: &SegmentCtx<'_>,
        start_position: Option<Duration>,
        play_duration: Duration,
    ) -> anyhow::Result<()> {
        let path = ctx.file_resolver().resolve(&self.filename).await?;
        let source = open_track_source(&path, start_position)?;
        let track = ctx.audio_processor().await.write().await.add_track(
            source,
            0.0,
            vec![
                TrackCommand::SetVolume {
                    volume: self.volume,
                    fade: FADE_TIME,
                    at: None,
                },
                TrackCommand::AddTrigger {
                    at: play_duration,
                    callback: self.done.clone().into(),
                },
                TrackCommand::Stop {
                    fade: FADE_TIME,
                    at: Some(play_duration),
                },
            ],
        );
        self.track.set(track);
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
    fn completed(&self) -> Completed {
        self.done.completed()
    }

    async fn restore(&self, ctx: &SegmentCtx) -> anyhow::Result<()> {
        let elapsed = self.started_at.elapsed();
        if elapsed >= self.duration {
            tracing::info!(
                "DJ track '{}' has already finished (elapsed: {:.1}s, duration: {:.1}s), will advance to next state",
                self.filename,
                elapsed.as_secs_f32(),
                self.duration.as_secs_f32()
            );
            self.done.complete();
            return Ok(());
        }
        let position = self.start_position.unwrap_or_default() + elapsed;
        if let Err(e) = self
            .start_at(ctx, Some(position), self.duration.saturating_sub(elapsed))
            .await
        {
            self.done.complete();
            return Err(e);
        }
        tracing::info!(
            "Restored DJ track '{}' at {:.1}s into the segment",
            self.filename,
            elapsed.as_secs_f32()
        );
        Ok(())
    }

    async fn enter(&self, ctx: &SegmentCtx) -> anyhow::Result<()> {
        self.start_at(ctx, self.start_position, self.duration)
            .await?;
        tracing::info!(
            "DJ starting track '{}' (duration: {:.1}s)",
            self.filename,
            self.duration.as_secs_f32()
        );
        Ok(())
    }

    async fn exit(&self, _ctx: &SegmentCtx) {
        if !self.done.is_completed() {
            self.track.stop(FADE_TIME);
        }
    }

    fn loggable_properties(&self) -> serde_json::Value {
        serde_json::json!({
            "filename": self.filename,
            "duration_secs": self.duration.as_secs_f32(),
            "forced_profile": self.signal_profile().map(|profile| profile.name),
        })
    }

    fn state_name(&self) -> &'static str {
        "playing_track"
    }

    fn state_display(&self) -> StateDisplay {
        StateDisplay {
            name: "PlayingTrack",
            subject: Some(self.filename.clone()),
            total: Some(self.duration),
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
