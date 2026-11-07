use super::track::with_status_emoji;
use super::{
    CommonSegmentPlaybackConfig, Segment, SegmentCtx, SegmentPlaybackConfig, StateDisplay,
    TrackEntry, TrackSegment,
};
use crate::audio::dj::config::SegmentDefaults;
use crate::audio::dj::segments::signals::{Completed, CompletedSignal};
use crate::audio::dj::weighted_choice::{Identity, Weighted};
use crate::audio::duration::DurationCache;
use crate::state::Data;
use async_trait::async_trait;
use field_trait::field_trait_impl;
use serde::{Deserialize, Serialize};
use serenity::model::id::GuildId;
use std::ops::Deref;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::task::JoinHandle;
use tracing::Instrument;

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct PlaylistEntry {
    pub tracks: Vec<String>,
    pub weight: u32,
    pub signal_profile: Option<String>,
    pub volume: Option<f32>,
    pub channel_status: Option<String>,
}

#[field_trait_impl]
impl Weighted for PlaylistEntry {
    fn identity(&self) -> Identity {
        Identity::of((
            &self.tracks,
            &self.signal_profile,
            self.volume.map(f32::to_bits),
            &self.channel_status,
        ))
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PlaylistSegment {
    config: CommonSegmentPlaybackConfig,
    tracks: Arc<[TrackSegment]>,
    #[serde(skip)]
    position: Arc<AtomicUsize>,
    #[serde(skip)]
    run: Mutex<Option<JoinHandle<()>>>,
    #[serde(skip)]
    done: CompletedSignal,
}

impl Deref for PlaylistSegment {
    type Target = CommonSegmentPlaybackConfig;
    fn deref(&self) -> &Self::Target {
        &self.config
    }
}

impl PlaylistSegment {
    pub async fn from_entry(
        entry: PlaylistEntry,
        segment_defaults: &SegmentDefaults,
        duration_cache: &DurationCache,
    ) -> Self {
        let mut tracks = Vec::with_capacity(entry.tracks.len());
        for filename in entry.tracks {
            let track = TrackEntry {
                filename,
                weight: 0,
                max_duration_seconds: None,
                allow_subsection: None,
                signal_profile: None,
                volume: entry.volume,
                channel_status: None,
            };
            tracks.push(
                TrackSegment::from_entry(track, &SegmentDefaults::default(), duration_cache).await,
            );
        }
        let config = CommonSegmentPlaybackConfig {
            signal_profile: entry
                .signal_profile
                .or_else(|| segment_defaults.signal_profile.clone())
                .map(Into::into),
            channel_status: entry
                .channel_status
                .as_deref()
                .map(with_status_emoji)
                .or_else(|| segment_defaults.channel_status.clone()),
        };
        Self {
            config,
            tracks: tracks.into(),
            position: Default::default(),
            run: Default::default(),
            done: Default::default(),
        }
    }

    fn total_duration(&self) -> Duration {
        self.tracks.iter().map(TrackSegment::duration).sum()
    }

    fn start_run(&self, ctx: &SegmentCtx<'_>, elapsed: Duration) {
        let handle = tokio::spawn(
            run_playlist(
                self.tracks.clone(),
                self.position.clone(),
                elapsed,
                *ctx.guild_id,
                ctx.bot_state(),
                self.done.clone(),
            )
            .instrument(tracing::Span::current()),
        );
        *self.run.lock().unwrap() = Some(handle);
    }

    fn filenames(&self) -> Vec<&str> {
        self.tracks.iter().map(TrackSegment::filename).collect()
    }
}

#[async_trait]
#[typetag::serde]
impl Segment for PlaylistSegment {
    fn completed(&self, _elapsed: Duration) -> Completed {
        self.done.completed()
    }

    async fn restore(&self, ctx: &SegmentCtx, elapsed: Duration) -> anyhow::Result<()> {
        self.start_run(ctx, elapsed);
        tracing::info!(
            "Restored DJ playlist at {:.1}s of {:.1}s",
            elapsed.as_secs_f32(),
            self.total_duration().as_secs_f32()
        );
        Ok(())
    }

    async fn enter(&self, ctx: &SegmentCtx) -> anyhow::Result<()> {
        self.start_run(ctx, Duration::ZERO);
        tracing::info!(
            "DJ starting playlist of {} tracks (duration: {:.1}s)",
            self.tracks.len(),
            self.total_duration().as_secs_f32()
        );
        Ok(())
    }

    async fn exit(&self, ctx: &SegmentCtx) {
        let run = self.run.lock().unwrap().take();
        if let Some(handle) = run {
            handle.abort();
            let _ = handle.await;
        }
        if let Some(track) = self.tracks.get(self.position.load(Ordering::Relaxed)) {
            track.exit(ctx).await;
        }
    }

    fn loggable_properties(&self) -> serde_json::Value {
        serde_json::json!({
            "tracks": self.filenames(),
            "duration_secs": self.total_duration().as_secs_f32(),
            "forced_profile": self.signal_profile().map(|profile| profile.name),
        })
    }

    fn state_name(&self) -> &'static str {
        "playing_playlist"
    }

    fn state_display(&self) -> StateDisplay {
        let position = self.position.load(Ordering::Relaxed);
        let subject = self.tracks.get(position).map(|track| {
            format!(
                "{} (track {}/{})",
                track.filename(),
                position + 1,
                self.tracks.len()
            )
        });
        StateDisplay {
            name: "PlayingPlaylist",
            subject,
            total: Some(self.total_duration()),
        }
    }
}

async fn run_playlist(
    tracks: Arc<[TrackSegment]>,
    position: Arc<AtomicUsize>,
    elapsed: Duration,
    guild_id: GuildId,
    bot_state: Data,
    done: CompletedSignal,
) {
    let ctx = SegmentCtx::new(&guild_id, bot_state);
    let mut offset = elapsed;
    for (index, track) in tracks.iter().enumerate() {
        if offset >= track.duration() && index + 1 < tracks.len() {
            offset -= track.duration();
            continue;
        }
        position.store(index, Ordering::Relaxed);
        let started = if offset.is_zero() {
            track.enter(&ctx).await
        } else {
            track.restore(&ctx, std::mem::take(&mut offset)).await
        };
        match started {
            Ok(()) => track.completed(Duration::ZERO).await,
            Err(e) => tracing::warn!(
                "Failed to play playlist track '{}': {e:#}",
                track.filename()
            ),
        }
    }
    done.complete();
}
