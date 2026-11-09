use super::{Segment, SegmentCtx, SegmentPlaybackConfig, StateDisplay};
use crate::audio::dj::announcement::Announcement;
use crate::audio::dj::config::{EntryPlaybackConfig, PoolEntry, SegmentDefaults};
use crate::audio::dj::segments::signals::{ChangedSignal, Completed, CompletedSignal};
use crate::audio::dj::signal_profile::SignalProfilePlayback;
use crate::audio::dj::weighted_choice::{Identity, Weighted};
use crate::state::Data;
use async_trait::async_trait;
use field_trait::field_trait_impl;
use serde::{Deserialize, Serialize};
use serenity::model::id::GuildId;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::task::JoinHandle;
use tracing::Instrument;

#[derive(Deserialize, Serialize, Debug)]
pub struct SequenceEntry {
    pub segments: Vec<PoolEntry>,
    pub weight: u32,
    #[serde(flatten)]
    pub playback: EntryPlaybackConfig,
}

#[field_trait_impl]
impl Weighted for SequenceEntry {
    fn identity(&self) -> Identity {
        Identity::of((
            self.segments
                .iter()
                .map(Weighted::identity)
                .collect::<Vec<_>>(),
            &self.playback,
        ))
    }
}

impl SequenceEntry {
    pub fn segment_defaults(&self, pool_defaults: &SegmentDefaults) -> SegmentDefaults {
        SegmentDefaults {
            hex_config: None,
            signal_profile: self.playback.signal_profile.clone(),
            channel_status: self.playback.channel_status.clone(),
            announcements: self.playback.announcement.iter().cloned().collect(),
        }
        .or(pool_defaults)
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct SequenceProgress {
    index: usize,
    child_offset: Duration,
}

#[derive(Debug, Serialize, Deserialize)]
struct SequenceChildren {
    segments: Vec<Box<dyn Segment>>,
    progress: Mutex<SequenceProgress>,
}

impl SequenceChildren {
    fn current(&self) -> Option<&dyn Segment> {
        let index = self.progress.lock().unwrap().index;
        self.segments.get(index).map(|segment| &**segment)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SequenceSegment {
    children: Arc<SequenceChildren>,
    #[serde(skip)]
    run: Mutex<Option<JoinHandle<()>>>,
    #[serde(skip)]
    done: CompletedSignal,
    #[serde(skip)]
    advanced: ChangedSignal,
}

impl SequenceSegment {
    pub fn new(segments: Vec<Box<dyn Segment>>) -> Self {
        Self {
            children: Arc::new(SequenceChildren {
                segments,
                progress: Default::default(),
            }),
            run: Default::default(),
            done: Default::default(),
            advanced: Default::default(),
        }
    }

    fn start_run(&self, ctx: &SegmentCtx<'_>, elapsed: Option<Duration>) {
        let handle = tokio::spawn(
            run_sequence(
                SequenceRun {
                    children: self.children.clone(),
                    guild_id: *ctx.guild_id,
                    bot_state: ctx.bot_state,
                    started_at: Instant::now()
                        .checked_sub(elapsed.unwrap_or_default())
                        .unwrap_or_else(Instant::now),
                    resume: elapsed.is_some(),
                },
                self.done.clone(),
                self.advanced.clone(),
            )
            .instrument(tracing::Span::current()),
        );
        *self.run.lock().unwrap() = Some(handle);
    }
}

impl SegmentPlaybackConfig for SequenceSegment {
    fn signal_profile(&self) -> Option<SignalProfilePlayback> {
        self.children.current()?.signal_profile()
    }

    fn channel_status(&self) -> Option<String> {
        self.children.current()?.channel_status()
    }

    fn announcement(&self) -> Option<Announcement> {
        self.children.current()?.announcement()
    }
}

#[async_trait]
#[typetag::serde]
impl Segment for SequenceSegment {
    fn completed(&self, _elapsed: Duration) -> Completed {
        self.done.completed()
    }

    fn changed(&self) -> Completed {
        let advanced = self.advanced.changed();
        let child = self
            .children
            .current()
            .map_or_else(Completed::never, |child| child.changed());
        Completed::when(async move {
            futures_util::future::select(advanced.into_future(), child.into_future()).await;
        })
    }

    async fn restore(&self, ctx: &SegmentCtx, elapsed: Duration) -> anyhow::Result<()> {
        self.start_run(ctx, Some(elapsed));
        tracing::info!(
            "Restored DJ sequence at segment {}/{}",
            self.children.progress.lock().unwrap().index + 1,
            self.children.segments.len()
        );
        Ok(())
    }

    async fn enter(&self, ctx: &SegmentCtx) -> anyhow::Result<()> {
        self.start_run(ctx, None);
        tracing::info!(
            "DJ playing sequence of {} segments",
            self.children.segments.len()
        );
        Ok(())
    }

    async fn exit(&self, ctx: &SegmentCtx) {
        let run = self.run.lock().unwrap().take();
        if let Some(handle) = run {
            handle.abort();
            let _ = handle.await;
        }
        if let Some(child) = self.children.current() {
            child.exit(ctx).await;
        }
    }

    fn loggable_properties(&self) -> serde_json::Value {
        serde_json::json!({
            "segments": self
                .children
                .segments
                .iter()
                .map(|child| serde_json::json!({
                    "state": child.state_name(),
                    "details": child.loggable_properties(),
                }))
                .collect::<Vec<_>>(),
        })
    }

    fn state_name(&self) -> &'static str {
        "playing_sequence"
    }

    fn state_display(&self) -> StateDisplay {
        let count = self.children.segments.len();
        let position = self.children.progress.lock().unwrap().index + 1;
        let subject = self.children.current().map(|child| {
            let display = child.state_display();
            match display.subject {
                Some(subject) => format!("{position}/{count} {}: {subject}", display.name),
                None => format!("{position}/{count} {}", display.name),
            }
        });
        StateDisplay {
            name: "PlayingSequence",
            subject,
            total: self
                .children
                .segments
                .iter()
                .map(|child| child.state_display().total)
                .sum(),
        }
    }
}

struct SequenceRun {
    children: Arc<SequenceChildren>,
    guild_id: GuildId,
    bot_state: Data,
    started_at: Instant,
    resume: bool,
}

async fn run_sequence(run: SequenceRun, done: CompletedSignal, advanced: ChangedSignal) {
    let SequenceRun {
        children,
        guild_id,
        bot_state,
        started_at,
        mut resume,
    } = run;
    let ctx = SegmentCtx::new(&guild_id, bot_state);
    loop {
        let (index, child_offset) = {
            let progress = children.progress.lock().unwrap();
            (progress.index, progress.child_offset)
        };
        let Some(child) = children.segments.get(index) else {
            break;
        };
        let child_elapsed = if resume {
            started_at.elapsed().saturating_sub(child_offset)
        } else {
            Duration::ZERO
        };
        let started = if resume {
            child.restore(&ctx, child_elapsed).await
        } else {
            child.enter(&ctx).await
        };
        resume = false;
        match started {
            Ok(()) => {
                advanced.notify();
                child.completed(child_elapsed).await;
                child.exit(&ctx).await;
            }
            Err(e) => tracing::warn!(
                "DJ sequence segment {}/{} ({}) failed: {e:#}",
                index + 1,
                children.segments.len(),
                child.state_name()
            ),
        }
        let mut progress = children.progress.lock().unwrap();
        progress.index = index + 1;
        progress.child_offset = started_at.elapsed();
    }
    done.complete();
}
