use super::{
    CommonSegmentPlaybackConfig, Segment, SegmentCtx, SegmentPlaybackConfig, StateDisplay,
    TimedSegment,
};
use crate::audio::dj::config::TrackEntry;
use crate::audio::duration::DurationCache;
use crate::audio::tracks::{StartTrackArgs, TrackManager};
use async_trait::async_trait;
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::ops::Deref;
use std::time::Duration;
use tokio::sync::Mutex;

pub fn format_dj_track_name(filename: &str) -> String {
    format!("dj_track_{}", filename)
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TrackSegment {
    timer: TimedSegment,
    filename: String,
    volume: f32,
    start_position: Option<Duration>,
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
        }
    }

    async fn start_at(
        &self,
        track_manager: &Mutex<TrackManager>,
        start_position: Option<Duration>,
    ) -> anyhow::Result<()> {
        track_manager
            .lock()
            .await
            .start_track(StartTrackArgs {
                name: format_dj_track_name(&self.filename),
                filename: self.filename.clone(),
                volume: self.volume,
                fade_time: 1.0,
                loops: false,
                start_position,
                persist: false,
            })
            .await
            .map(drop)
            .map_err(|e| anyhow::anyhow!(e))
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
        self.timer.is_elapsed()
    }

    fn elapsed(&self) -> Duration {
        self.timer.started_at.elapsed()
    }

    async fn restore(&self, ctx: &SegmentCtx) -> anyhow::Result<()> {
        let elapsed = self.elapsed();
        if self.timer.is_elapsed() {
            tracing::info!(
                "DJ track '{}' has already finished (elapsed: {:.1}s, duration: {:.1}s), will advance to next state",
                format_dj_track_name(&self.filename),
                elapsed.as_secs_f32(),
                self.timer.duration.as_secs_f32()
            );
            return Ok(());
        }
        let position = self.start_position.unwrap_or_default() + elapsed;
        self.start_at(ctx.track_manager, Some(position)).await?;
        tracing::info!(
            "Restored DJ track '{}' at {:.1}s into the segment in guild {}",
            format_dj_track_name(&self.filename),
            elapsed.as_secs_f32(),
            ctx.guild_id
        );
        Ok(())
    }

    async fn enter(&self, ctx: &SegmentCtx) -> anyhow::Result<()> {
        self.start_at(ctx.track_manager, self.start_position)
            .await?;
        tracing::info!(
            "DJ starting track '{}' (duration: {:.1}s) in guild {}",
            self.filename,
            self.timer.duration.as_secs_f32(),
            ctx.guild_id
        );
        Ok(())
    }

    async fn exit(&self, ctx: &SegmentCtx) -> anyhow::Result<()> {
        let track_name = format_dj_track_name(&self.filename);
        let mut track_manager = ctx.track_manager.lock().await;
        if track_manager.has_track(&track_name) {
            track_manager
                .stop_track(&track_name, 1.0, false)
                .await
                .map_err(|e| anyhow::anyhow!(e))?;
        }
        Ok(())
    }

    fn loggable_properties(&self) -> serde_json::Value {
        serde_json::json!({
            "track_name": format_dj_track_name(&self.filename),
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
