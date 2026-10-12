use super::{
    CommonSegmentPlaybackConfig, Segment, SegmentCtx, SegmentPlaybackConfig, StateDisplay,
};
use crate::audio::dj::serde::{deser_instant, ser_instant};
use crate::audio::tracks::{StartTrackArgs, TrackManager};
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
use tokio::sync::{Mutex, Notify};
use tokio::task::JoinHandle;

const HEX_TRACK_NAME: &str = "hex_playback";

#[derive(Debug, Default, Serialize, Deserialize)]
struct HexProgress {
    position: AtomicUsize,
    loops: AtomicUsize,
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
            progress: Default::default(),
            run: Default::default(),
        }
    }

    fn start_run(&self, ctx: &SegmentCtx) {
        let handle = tokio::spawn(run_hex_message(
            self.message.clone(),
            self.volume,
            self.target_loops,
            self.progress.clone(),
            ctx.track_manager.clone(),
            ctx.bot_state.file_resolver.clone(),
            *ctx.guild_id,
        ));
        *self.run.lock().unwrap() = Some(handle);
    }
}

#[async_trait]
#[typetag::serde]
impl Segment for HexSegment {
    async fn is_complete(&self, _ctx: &SegmentCtx) -> bool {
        self.run
            .lock()
            .unwrap()
            .as_ref()
            .is_none_or(JoinHandle::is_finished)
    }

    fn elapsed(&self) -> Duration {
        self.started_at.elapsed()
    }

    async fn restore(&self, ctx: &SegmentCtx) -> anyhow::Result<()> {
        self.start_run(ctx);
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
        self.start_run(ctx);
        tracing::info!(
            "DJ playing hex message '{}' ({} loops) in guild {}",
            self.message,
            self.target_loops,
            ctx.guild_id
        );
        Ok(())
    }

    async fn exit(&self, ctx: &SegmentCtx) -> anyhow::Result<()> {
        let handle = self.run.lock().unwrap().take();
        if let Some(handle) = handle {
            handle.abort();
            let _ = handle.await;
        }
        let mut track_manager = ctx.track_manager.lock().await;
        if track_manager.has_track(HEX_TRACK_NAME) {
            track_manager.remove_track(HEX_TRACK_NAME).await;
        }
        drop(track_manager);
        if let Some(metrics) = ctx.bot_state.metrics.read().await.as_ref() {
            metrics.record_hex_message_loops(ctx.guild_id.get(), self.target_loops as u64);
        }
        Ok(())
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
        StateDisplay {
            name: "PlayingHexMessage",
            subject: Some(format!(
                "{} (loop {current_loop}/{})",
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
    track_manager: Arc<Mutex<TrackManager>>,
    file_resolver: FileResolver,
    guild_id: GuildId,
) {
    let hex_chars = message_to_hex_sequence(&message);
    while !hex_chars.is_empty() && progress.loops.load(Ordering::Relaxed) < target_loops {
        let position = progress.position.load(Ordering::Relaxed);
        let filename = format!("audio/hex/hex_{}.wav", hex_chars[position]);
        match file_resolver.resolve(&filename).await {
            Ok(path) if Path::new(&path).exists() => {
                if let Err(e) = play_hex_char(&track_manager, filename, volume).await {
                    tracing::warn!("Failed to start hex track for message '{message}': {e:#}");
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    continue;
                }
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

async fn play_hex_char(
    track_manager: &Mutex<TrackManager>,
    filename: String,
    volume: f32,
) -> anyhow::Result<()> {
    let notify = Arc::new(Notify::new());
    let notify_clone = notify.clone();
    {
        let mut manager = track_manager.lock().await;
        if manager.has_track(HEX_TRACK_NAME) {
            manager.remove_track(HEX_TRACK_NAME).await;
        }
        manager
            .start_track_with_callback(
                StartTrackArgs {
                    name: HEX_TRACK_NAME.to_string(),
                    filename,
                    volume,
                    fade_time: 0.0,
                    loops: false,
                    start_position: None,
                    persist: false,
                },
                Some(Arc::new(move |_| notify_clone.notify_one())),
            )
            .await
            .map_err(|e| anyhow::anyhow!(e))?;
    }
    notify.notified().await;
    Ok(())
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
