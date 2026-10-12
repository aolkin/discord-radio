use super::{CommonSegmentPlaybackConfig, TimedSegment};
use crate::audio::dj::state_machine::format_dj_track_name;
use crate::audio::tracks::{StartTrackArgs, TrackManager};
use std::ops::Deref;
use std::time::Duration;
use tokio::sync::Mutex;

#[derive(Debug)]
pub struct TrackSegment {
    timer: TimedSegment,
    filename: String,
    volume: f32,
    start_position: Option<Duration>,
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
            .map_err(|e| anyhow::anyhow!(e))
    }

    pub async fn start(&self, track_manager: &Mutex<TrackManager>) -> anyhow::Result<()> {
        self.start_at(track_manager, self.start_position).await
    }

    pub async fn resume(
        &self,
        track_manager: &Mutex<TrackManager>,
        elapsed: Duration,
    ) -> anyhow::Result<()> {
        let position = self.start_position.unwrap_or_default() + elapsed;
        self.start_at(track_manager, Some(position)).await
    }

    pub async fn stop_file(
        track_manager: &Mutex<TrackManager>,
        filename: &str,
    ) -> anyhow::Result<()> {
        let track_name = format_dj_track_name(filename);
        let mut track_manager = track_manager.lock().await;
        if track_manager.has_track(&track_name) {
            track_manager
                .stop_track(&track_name, 1.0, false)
                .await
                .map_err(|e| anyhow::anyhow!(e))?;
        }
        Ok(())
    }
}

impl Deref for TrackSegment {
    type Target = CommonSegmentPlaybackConfig;
    fn deref(&self) -> &Self::Target {
        &self.timer
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
