use super::{
    CommonSegmentPlaybackConfig, Segment, SegmentCtx, SegmentPlaybackConfig, StateDisplay,
    TrackSlot,
};
use crate::audio::dj::segments::signals::{Completed, CompletedSignal};
use crate::audio::dj::weighted_choice::Weighted;
use crate::audio::processing::mixer::{TrackCallback, TrackCommand};
use crate::audio::processing::processor::AudioProcessor;
use crate::audio::tracks::open_track_source;
use crate::bucket::FileResolver;
use anyhow::bail;
use async_trait::async_trait;
use field_traits::field_trait_impl;
use rand::RngExt;
use rand::seq::IndexedRandom;
use serde::{Deserialize, Serialize};
use std::ops::Deref;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::task::JoinHandle;
use tracing::Instrument;

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct HexMessageDefaults {
    #[serde(default = "default_loops")]
    pub loop_min: u32,
    #[serde(default = "default_loops")]
    pub loop_max: u32,
    pub signal_profile: Option<String>,
    #[serde(default)]
    pub announcements: Vec<String>,
}

impl Default for HexMessageDefaults {
    fn default() -> Self {
        Self {
            loop_min: default_loops(),
            loop_max: default_loops(),
            signal_profile: None,
            announcements: Vec::new(),
        }
    }
}

fn default_loops() -> u32 {
    1
}

impl HexMessageDefaults {
    pub fn pick_random_announcement(&self) -> Option<String> {
        self.announcements.choose(&mut rand::rng()).cloned()
    }
}

#[field_trait_impl(Weighted)]
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct HexMessageEntry {
    pub text: String,
    pub weight: u32,
    pub signal_profile: Option<String>,
    pub loop_min: Option<u32>,
    pub loop_max: Option<u32>,
    pub announcement: Option<String>,
}

pub fn ensure_loops(min: u32, max: u32) -> anyhow::Result<()> {
    if min > max {
        bail!("loop_min must not exceed loop_max");
    }
    Ok(())
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct HexProgress {
    position: AtomicUsize,
    loops: AtomicUsize,
    #[serde(skip)]
    current_track: TrackSlot,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct HexSegment {
    playback_config: CommonSegmentPlaybackConfig,
    message: String,
    volume: f32,
    target_loops: usize,
    progress: Arc<HexProgress>,
    announcement: Option<String>,
    #[serde(skip)]
    run: Mutex<Option<JoinHandle<()>>>,
    #[serde(skip)]
    done: CompletedSignal,
}

impl Deref for HexSegment {
    type Target = CommonSegmentPlaybackConfig;
    fn deref(&self) -> &Self::Target {
        &self.playback_config
    }
}

impl HexSegment {
    pub const STATE_NAME: &'static str = "playing_hex_message";

    pub fn new(
        message: String,
        volume: f32,
        target_loops: usize,
        announcement: Option<String>,
        signal_profile: Option<String>,
    ) -> Self {
        let playback_config = CommonSegmentPlaybackConfig {
            signal_profile: signal_profile.map(Into::into),
            channel_status: Some(obfuscate_message(&message)),
        };
        Self {
            playback_config,
            message,
            volume,
            target_loops,
            announcement,
            progress: Default::default(),
            run: Default::default(),
            done: Default::default(),
        }
    }

    async fn start_run(&self, ctx: &SegmentCtx<'_>) {
        let processor = ctx.audio_processor().await;
        let handle = tokio::spawn(
            run_hex_message(
                self.message.clone(),
                self.volume,
                self.target_loops,
                self.progress.clone(),
                processor,
                ctx.file_resolver().clone(),
                self.done.clone(),
            )
            .instrument(tracing::Span::current()),
        );
        *self.run.lock().unwrap() = Some(handle);
    }

    pub fn from_entry(hex_entry: HexMessageEntry, defaults: &HexMessageDefaults) -> Self {
        let loop_min = hex_entry.loop_min.unwrap_or(defaults.loop_min);
        let loop_max = hex_entry.loop_max.unwrap_or(defaults.loop_max);
        let signal_profile = hex_entry
            .signal_profile
            .or_else(|| defaults.signal_profile.clone());
        let announcement = hex_entry
            .announcement
            .or_else(|| defaults.pick_random_announcement());
        let loops = rand::rng().random_range(loop_min..=loop_max) as usize;

        Self::new(hex_entry.text, 1.0, loops, announcement, signal_profile)
    }
}

#[async_trait]
#[typetag::serde]
impl Segment for HexSegment {
    fn completed(&self, _elapsed: Duration) -> Completed {
        self.done.completed()
    }

    async fn restore(&self, ctx: &SegmentCtx, _elapsed: Duration) -> anyhow::Result<()> {
        self.start_run(ctx).await;
        tracing::info!(
            "Restored DJ hex message '{}' at loop {}/{}",
            self.message,
            self.progress.loops.load(Ordering::Relaxed),
            self.target_loops
        );
        Ok(())
    }

    async fn enter(&self, ctx: &SegmentCtx) -> anyhow::Result<()> {
        self.start_run(ctx).await;
        tracing::info!(
            "DJ playing hex message '{}' ({} loops)",
            self.message,
            self.target_loops
        );
        Ok(())
    }

    async fn exit(&self, ctx: &SegmentCtx) {
        let run = self.run.lock().unwrap().take();
        if let Some(handle) = run {
            handle.abort();
            let _ = handle.await;
        }
        self.progress.current_track.stop(Duration::ZERO);
        if let Some(metrics) = crate::metrics::get() {
            let loops_played = self.progress.loops.load(Ordering::Relaxed) as u64;
            metrics.record_hex_message_loops(ctx.guild_id.get(), loops_played);
        }
    }

    fn announcement(&self) -> &Option<String> {
        &self.announcement
    }

    fn loggable_properties(&self) -> serde_json::Value {
        serde_json::json!({
            "message": self.message,
            "target_loops": self.target_loops,
            "forced_profile": self.signal_profile().map(|profile| profile.name),
        })
    }

    fn state_name(&self) -> &'static str {
        Self::STATE_NAME
    }

    fn state_display(&self) -> StateDisplay {
        let current_loop = (self.progress.loops.load(Ordering::Relaxed) + 1).min(self.target_loops);
        let hex_len = self.message.len() * 2;
        let current_char = (self.progress.position.load(Ordering::Relaxed) + 1).min(hex_len);
        StateDisplay {
            name: "PlayingHexMessage",
            subject: Some(format!(
                "{} (loop {current_loop}/{}, char {current_char}/{hex_len})",
                self.message, self.target_loops
            )),
            total: None,
        }
    }
}

async fn run_hex_message(
    message: String,
    volume: f32,
    target_loops: usize,
    progress: Arc<HexProgress>,
    processor: Arc<RwLock<AudioProcessor>>,
    file_resolver: FileResolver,
    done: CompletedSignal,
) {
    let hex_chars = message_to_hex_sequence(&message);
    while !hex_chars.is_empty() && progress.loops.load(Ordering::Relaxed) < target_loops {
        let position = progress.position.load(Ordering::Relaxed);
        let filename = format!("audio/hex/hex_{}.wav", hex_chars[position]);
        match file_resolver.resolve(&filename).await {
            Ok(path) if Path::new(&path).exists() => {
                let source = match open_track_source(&path, None) {
                    Ok(source) => source,
                    Err(e) => {
                        tracing::warn!("Failed to start hex track for message '{message}': {e:#}");
                        tokio::time::sleep(Duration::from_millis(100)).await;
                        continue;
                    }
                };
                let (ended_tx, ended) = TrackCallback::oneshot();
                let track = processor.write().await.add_track(
                    source,
                    volume,
                    vec![TrackCommand::AddStopTrigger(ended_tx)],
                );
                progress.current_track.set(track);
                let _ = ended.await;
            }
            _ => tracing::warn!("Audio file not found: {}", filename),
        }

        let next_position = position + 1;
        if next_position < hex_chars.len() {
            progress.position.store(next_position, Ordering::Relaxed);
        } else {
            progress.position.store(0, Ordering::Relaxed);
            let loops = progress.loops.fetch_add(1, Ordering::Relaxed) + 1;
            tracing::info!("Hex message '{message}' completed loop {loops}/{target_loops}");
            if loops >= target_loops {
                break;
            }
        }

        let delay = if next_position.is_multiple_of(2) {
            Duration::from_millis(800)
        } else {
            Duration::from_millis(50)
        };
        tokio::time::sleep(delay).await;
    }
    done.complete();
}

fn message_to_hex_sequence(message: &str) -> Vec<char> {
    message
        .bytes()
        .flat_map(|byte| format!("{byte:02X}").chars().collect::<Vec<_>>())
        .collect()
}

fn obfuscate_message(message: &str) -> String {
    let mut rng = rand::rng();
    message
        .chars()
        .map(|c| {
            if c.is_alphanumeric() {
                if rng.random_bool(0.5) { '?' } else { '¿' }
            } else {
                c
            }
        })
        .collect()
}
