use super::{
    CommonSegmentPlaybackConfig, Segment, SegmentCtx, SegmentPlaybackConfig, StateDisplay,
    TrackSlot,
};
use crate::audio::dj::config::{HexMessageDefaults, HexMessageEntry};
use crate::audio::dj::serde::{deser_instant, ser_instant};
use crate::audio::mixer::TrackCommand;
use crate::audio::processing_thread::AudioProcessor;
use crate::audio::tracks::open_track_source;
use crate::bucket::FileResolver;
use async_trait::async_trait;
use rand::Rng;
use serde::{Deserialize, Serialize};
use serenity::model::id::GuildId;
use std::ops::Deref;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tokio::task::JoinHandle;

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
    #[serde(serialize_with = "ser_instant", deserialize_with = "deser_instant")]
    started_at: Instant,
    message: String,
    volume: f32,
    target_loops: usize,
    progress: Arc<HexProgress>,
    announcement: Option<String>,
    #[serde(skip)]
    run: std::sync::Mutex<Option<JoinHandle<()>>>,
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
            started_at: Instant::now(),
            message,
            volume,
            target_loops,
            announcement,
            progress: Default::default(),
            run: Default::default(),
        }
    }

    async fn start_run(&self, ctx: &SegmentCtx<'_>) -> anyhow::Result<()> {
        let processor = ctx.audio_processor().await?;
        let handle = tokio::spawn(run_hex_message(
            self.message.clone(),
            self.volume,
            self.target_loops,
            self.progress.clone(),
            processor,
            ctx.bot_state.file_resolver.clone(),
            *ctx.guild_id,
        ));
        *self.run.lock().unwrap() = Some(handle);
        Ok(())
    }

    pub fn from_entry(
        hex_entry: HexMessageEntry,
        defaults: &HexMessageDefaults,
        default_announcement: impl FnOnce() -> Option<String>,
    ) -> Self {
        let loop_min = hex_entry.loop_min.unwrap_or(defaults.loop_min);
        let loop_max = hex_entry.loop_max.unwrap_or(defaults.loop_max);
        let signal_profile = hex_entry
            .signal_profile
            .or_else(|| defaults.signal_profile.clone());
        let announcement = hex_entry.announcement.or_else(default_announcement);
        let loops = rand::rng().random_range(loop_min..=loop_max) as usize;

        Self::new(hex_entry.text, 1.0, loops, announcement, signal_profile)
    }
}

#[async_trait]
#[typetag::serde]
impl Segment for HexSegment {
    async fn is_complete(&self) -> bool {
        self.run
            .lock()
            .unwrap()
            .as_ref()
            .is_none_or(|handle| handle.is_finished())
    }

    fn elapsed(&self) -> Duration {
        self.started_at.elapsed()
    }

    async fn restore(&self, ctx: &SegmentCtx) -> anyhow::Result<()> {
        self.start_run(ctx).await?;
        tracing::info!(
            "Restored DJ hex message '{}' at loop {}/{} in guild {}",
            self.message,
            self.progress.loops.load(Ordering::Relaxed),
            self.target_loops,
            ctx.guild_id
        );
        Ok(())
    }

    async fn enter(&self, ctx: &SegmentCtx) -> anyhow::Result<()> {
        self.start_run(ctx).await?;
        tracing::info!(
            "DJ playing hex message '{}' ({} loops) in guild {}",
            self.message,
            self.target_loops,
            ctx.guild_id
        );
        Ok(())
    }

    async fn exit(&self, ctx: &SegmentCtx) -> anyhow::Result<()> {
        let run = self.run.lock().unwrap().take();
        if let Some(handle) = run {
            handle.abort();
            let _ = handle.await;
        }
        self.progress.current_track.stop(Duration::ZERO);
        if let Some(metrics) = ctx.bot_state.metrics.read().await.as_ref() {
            let loops_played = self.progress.loops.load(Ordering::Relaxed) as u64;
            metrics.record_hex_message_loops(ctx.guild_id.get(), loops_played);
        }
        Ok(())
    }

    fn announcement(&self) -> &Option<String> {
        &self.announcement
    }

    fn loggable_properties(&self) -> serde_json::Value {
        serde_json::json!({
            "message": self.message,
            "target_loops": self.target_loops,
            "forced_profile": self.signal_profile().name,
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
            elapsed: self.elapsed(),
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
    guild_id: GuildId,
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
                let (stop_trigger, ended) = TrackCommand::add_stop_trigger();
                let track = processor.write().await.mixer_mut().add_track(
                    source,
                    volume,
                    vec![stop_trigger],
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
            tracing::info!(
                "Hex message '{message}' completed loop {loops}/{target_loops} in guild {guild_id}"
            );
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
