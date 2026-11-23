use super::{
    CommonSegmentPlaybackConfig, Segment, SegmentCtx, SegmentPlaybackConfig, StateDisplay,
    TrackSlot,
};
use crate::audio::dj::config::{EntryPlaybackConfig, SegmentDefaults};
use crate::audio::dj::encoding::{EncodedMessage, EncodedStep, TtsEncoding};
use crate::audio::dj::segments::signals::{ChangedSignal, Completed, CompletedSignal};
use crate::audio::dj::weighted_choice::{Identity, Weighted};
use crate::audio::processing::mixer::{TrackCallback, TrackCommand};
use crate::audio::processing::processor::AudioProcessor;
use crate::audio::tracks::open_track_source;
use crate::bucket::{FileResolver, with_trailing_slash};
use anyhow::{Context, bail};
use async_trait::async_trait;
use field_trait::field_trait_impl;
use rand::RngExt;
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
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_audio_folder"
    )]
    pub hex_audio_folder: Option<String>,
}

impl Default for HexMessageDefaults {
    fn default() -> Self {
        Self {
            loop_min: default_loops(),
            loop_max: default_loops(),
            hex_audio_folder: None,
        }
    }
}

fn default_loops() -> u32 {
    1
}

impl HexMessageDefaults {
    pub fn validate(&self) -> anyhow::Result<()> {
        ensure_loops(self.loop_min, self.loop_max)
    }

    pub fn pick_loops(&self, loop_min: Option<u32>, loop_max: Option<u32>) -> u32 {
        rand::rng()
            .random_range(loop_min.unwrap_or(self.loop_min)..=loop_max.unwrap_or(self.loop_max))
    }
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct HexMessageEntry {
    pub text: String,
    pub weight: u32,
    pub loop_min: Option<u32>,
    pub loop_max: Option<u32>,
    #[serde(flatten)]
    pub playback: EntryPlaybackConfig,
}

#[field_trait_impl]
impl Weighted for HexMessageEntry {
    fn identity(&self) -> Identity {
        Identity::of((&self.text, self.loop_min, self.loop_max, &self.playback))
    }
}

fn deserialize_audio_folder<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<String>, D::Error> {
    Ok(Option::<String>::deserialize(deserializer)?.map(|folder| with_trailing_slash(&folder)))
}

const ENCODING: TtsEncoding = TtsEncoding::Hex;

fn encode_message(message: &str) -> anyhow::Result<EncodedMessage> {
    ENCODING
        .encode(message)
        .with_context(|| format!("encoding message {message:?}"))
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
    audio_folder: Option<String>,
    target_loops: usize,
    progress: Arc<HexProgress>,
    #[serde(skip)]
    task: Mutex<Option<JoinHandle<()>>>,
    #[serde(skip)]
    done: CompletedSignal,
    #[serde(skip)]
    progressed: ChangedSignal,
}

impl Deref for HexSegment {
    type Target = CommonSegmentPlaybackConfig;
    fn deref(&self) -> &Self::Target {
        &self.playback_config
    }
}

impl HexSegment {
    pub fn new(
        message: String,
        volume: f32,
        target_loops: usize,
        playback: EntryPlaybackConfig,
        segment_defaults: &SegmentDefaults,
    ) -> Self {
        let hint = encode_message(&message)
            .map(|encoded| encoded.hint)
            .inspect_err(|e| tracing::warn!("Failed to build hint: {e:#}"))
            .ok();
        let playback_config = segment_defaults.playback_config(EntryPlaybackConfig {
            channel_status: playback.channel_status.or(hint),
            ..playback
        });
        Self {
            playback_config,
            message,
            volume,
            audio_folder: segment_defaults.hex_config().hex_audio_folder.clone(),
            target_loops,
            progress: Default::default(),
            task: Default::default(),
            done: Default::default(),
            progressed: Default::default(),
        }
    }

    async fn start_playing(&self, ctx: &SegmentCtx<'_>) {
        let processor = ctx.audio_processor().clone();
        let handle = tokio::spawn(
            play_message(
                PlaySettings {
                    message: self.message.clone(),
                    volume: self.volume,
                    target_loops: self.target_loops,
                    audio_folder: self
                        .audio_folder
                        .clone()
                        .unwrap_or_else(|| ctx.default_hex_audio_folder().to_string()),
                },
                self.progress.clone(),
                processor,
                ctx.file_resolver().clone(),
                self.done.clone(),
                self.progressed.clone(),
            )
            .instrument(tracing::Span::current()),
        );
        *self.task.lock().unwrap() = Some(handle);
    }

    pub fn from_entry(hex_entry: HexMessageEntry, segment_defaults: &SegmentDefaults) -> Self {
        let defaults = segment_defaults.hex_config();
        let loops = defaults.pick_loops(hex_entry.loop_min, hex_entry.loop_max) as usize;
        Self::new(
            hex_entry.text,
            1.0,
            loops,
            hex_entry.playback,
            segment_defaults,
        )
    }
}

#[async_trait]
#[typetag::serde(name = "hex_message")]
impl Segment for HexSegment {
    fn completed(&self, _elapsed: Duration) -> Completed {
        self.done.completed()
    }

    fn changed(&self) -> Completed {
        self.progressed.changed()
    }

    async fn restore(&self, ctx: &SegmentCtx, _elapsed: Duration) -> anyhow::Result<()> {
        self.start_playing(ctx).await;
        tracing::info!(
            "Restored DJ hex message '{}' at loop {}/{}",
            self.message,
            self.progress.loops.load(Ordering::Relaxed),
            self.target_loops
        );
        Ok(())
    }

    async fn enter(&self, ctx: &SegmentCtx) -> anyhow::Result<()> {
        self.start_playing(ctx).await;
        tracing::info!(
            "DJ playing hex message '{}' ({} loops)",
            self.message,
            self.target_loops
        );
        Ok(())
    }

    async fn exit(&self, ctx: &SegmentCtx) {
        let task = self.task.lock().unwrap().take();
        if let Some(handle) = task {
            handle.abort();
            let _ = handle.await;
        }
        self.progress.current_track.stop(Duration::ZERO);
        if let Some(metrics) = crate::metrics::get() {
            let loops_played = self.progress.loops.load(Ordering::Relaxed) as u64;
            metrics.record_hex_message_loops(ctx.guild_id.get(), loops_played);
        }
    }

    fn loggable_properties(&self) -> serde_json::Value {
        serde_json::json!({
            "message": self.message,
            "target_loops": self.target_loops,
            "signal_preset": self.signal_preset().map(|preset| preset.name),
        })
    }

    fn state_display(&self) -> StateDisplay {
        let current_loop = (self.progress.loops.load(Ordering::Relaxed) + 1).min(self.target_loops);
        let encoded = encode_message(&self.message).unwrap_or_else(|_| EncodedMessage {
            hint: String::new(),
            steps: Vec::new(),
        });
        let sound_total = encoded.sound_count();
        let position = self.progress.position.load(Ordering::Relaxed);
        let sounds_played = encoded.steps[..position.min(encoded.steps.len())]
            .iter()
            .filter(|step| matches!(step, EncodedStep::Sound(_)))
            .count();
        let current_char = (sounds_played + 1).min(sound_total);
        StateDisplay {
            subject: Some(format!(
                "{} (loop {current_loop}/{}, char {current_char}/{sound_total})",
                self.message, self.target_loops
            )),
            total: None,
        }
    }
}

struct PlaySettings {
    message: String,
    volume: f32,
    target_loops: usize,
    audio_folder: String,
}

async fn play_message(
    settings: PlaySettings,
    progress: Arc<HexProgress>,
    processor: Arc<RwLock<AudioProcessor>>,
    file_resolver: FileResolver,
    done: CompletedSignal,
    progressed: ChangedSignal,
) {
    let PlaySettings {
        message,
        volume,
        target_loops,
        audio_folder,
    } = settings;
    let steps = match encode_message(&message) {
        Ok(encoded) => encoded.steps,
        Err(e) => {
            tracing::warn!("Failed to encode hex message: {e:#}");
            done.complete();
            return;
        }
    };
    while !steps.is_empty() && progress.loops.load(Ordering::Relaxed) < target_loops {
        let position = progress.position.load(Ordering::Relaxed);
        match steps[position] {
            EncodedStep::Pause(duration) => tokio::time::sleep(duration).await,
            EncodedStep::Sound(sound) => {
                let filename = format!("{audio_folder}{}", ENCODING.file_name(sound));
                match file_resolver.resolve(&filename).await {
                    Ok(path) if Path::new(&path).exists() => {
                        let source = match open_track_source(&path, None) {
                            Ok(source) => source,
                            Err(e) => {
                                tracing::warn!(
                                    "Failed to start hex track for message '{message}': {e:#}"
                                );
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
            }
        }

        let next_position = position + 1;
        if next_position < steps.len() {
            progress.position.store(next_position, Ordering::Relaxed);
        } else {
            progress.position.store(0, Ordering::Relaxed);
            let loops = progress.loops.fetch_add(1, Ordering::Relaxed) + 1;
            tracing::info!("Hex message '{message}' completed loop {loops}/{target_loops}");
            if loops >= target_loops {
                break;
            }
        }
        progressed.notify();
    }
    done.complete();
}
