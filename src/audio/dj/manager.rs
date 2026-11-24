use crate::audio::dj::config::{LoadedConfig, optional_seconds};
use crate::audio::dj::signal_preset::{SignalPresetPlayback, default_preset_fade};
use crate::audio::dj::state_machine::{ChannelStatus, DJState, DJStateMachine};
use crate::audio::dj::tiered::{ConfigurableTier, TimedValue};
use crate::audio::dj::tts::TtsEncoding;
use crate::audio::processing::processor::ProcessorRef;
use crate::logging::guild_span;
use crate::state::Data;
use anyhow::{Context, bail};
use serde::Deserialize;
use serenity::model::id::{ChannelId, GuildId};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc::error::TrySendError;
use tokio::sync::{RwLock, mpsc, watch};
use tokio::task::JoinHandle;
use tracing::Instrument;

pub const ADVANCE_RETRY_DELAY: Duration = Duration::from_millis(100);
const STOP_SEND_TIMEOUT: Duration = Duration::from_secs(1);

#[derive(strum::IntoStaticStr)]
pub enum DJCommand {
    ForceAdvance(Option<String>),
    ForceTtsMessage(TtsMessageRequest),
    ForceTrack(TrackRequest),
    ForceIdle(IdleRequest),
    Stop,
    SetConfig(watch::Receiver<Arc<LoadedConfig>>),
    AddAnnouncementChannel(ChannelId),
    RemoveAnnouncementChannel(ChannelId),
    SetSignalPreset(ConfigurableTier, Option<TimedValue<SignalPresetPlayback>>),
    SetChannelStatus(ConfigurableTier, Option<TimedValue<ChannelStatus>>),
}

#[derive(Debug, thiserror::Error)]
#[error("signal preset {0:?} not found")]
pub struct PresetNotFound(pub String);

pub fn ensure_pool(bot_state: Data, pool: Option<&str>) -> anyhow::Result<()> {
    if let Some(pool) = pool {
        bot_state.dj_config_manager.pools.require(pool)?;
    }
    Ok(())
}

pub fn ensure_preset(bot_state: Data, preset: Option<&str>) -> anyhow::Result<()> {
    if let Some(preset) = preset
        && bot_state.preset_manager.get(preset).is_none()
    {
        return Err(PresetNotFound(preset.to_string()).into());
    }
    Ok(())
}

/// A TTS message for the DJ to play, with TTS defaults from the pool named
/// `pool` when set. `volume`, `loops` and `encoding` replace the defaults when set.
#[derive(Debug, Deserialize)]
pub struct TtsMessageRequest {
    pub message: String,
    pub volume: Option<f32>,
    pub loops: Option<u32>,
    pub encoding: Option<TtsEncoding>,
    pub pool: Option<String>,
}

impl TtsMessageRequest {
    pub fn validate(&self, bot_state: Data) -> anyhow::Result<()> {
        let pool_encoding = match self.pool.as_deref() {
            Some(pool) => {
                bot_state
                    .dj_config_manager
                    .pools
                    .require(pool)?
                    .segment_defaults
                    .tts_config()
                    .encoding
            }
            None => TtsEncoding::default(),
        };
        self.encoding
            .unwrap_or(pool_encoding)
            .encode("", &self.message)
            .with_context(|| format!("TTS message {:?}", self.message))?;
        Ok(())
    }
}

/// A track for the DJ to play. `start_position` and `duration` are in seconds.
#[derive(Debug, Deserialize)]
pub struct TrackRequest {
    pub filename: String,
    pub volume: Option<f32>,
    pub start_position: Option<f32>,
    pub duration: Option<f32>,
}

impl TrackRequest {
    pub fn validate(&self) -> anyhow::Result<()> {
        optional_seconds(self.start_position, "start_position")?;
        optional_seconds(self.duration, "duration")?;
        Ok(())
    }
}

/// An idle segment for the DJ to enter, forcing `signal_preset` when set.
/// `duration` is in seconds; without it the segment never ends on its own.
#[derive(Debug, Deserialize)]
pub struct IdleRequest {
    pub signal_preset: Option<String>,
    pub duration: Option<f32>,
}

impl IdleRequest {
    pub fn validate(&self, bot_state: Data) -> anyhow::Result<()> {
        optional_seconds(self.duration, "duration")?;
        ensure_preset(bot_state, self.signal_preset.as_deref())
    }
}

#[derive(Debug, Deserialize)]
pub struct ConfigureRequest {
    pub config: String,
}

impl ConfigureRequest {
    pub fn into_command(self, bot_state: Data) -> anyhow::Result<DJCommand> {
        let config = bot_state.dj_config_manager.subscribe(&self.config)?;
        Ok(DJCommand::SetConfig(config))
    }
}

/// Sets (or, with `preset` unset, clears) the signal preset of `tier`.
/// `fade_duration` and `duration` are in seconds and apply only when setting.
#[derive(Debug, Deserialize)]
pub struct SignalPresetRequest {
    pub tier: ConfigurableTier,
    pub preset: Option<String>,
    pub fade_duration: Option<f32>,
    pub duration: Option<f32>,
}

impl SignalPresetRequest {
    pub fn into_command(self, bot_state: Data) -> anyhow::Result<DJCommand> {
        ensure_preset(bot_state, self.preset.as_deref())?;
        let fade = optional_seconds(self.fade_duration, "fade_duration")?
            .or(default_preset_fade(self.tier));
        let duration = optional_seconds(self.duration, "duration")?;
        let playback = self.preset.map(|name| {
            TimedValue::new(
                SignalPresetPlayback {
                    name,
                    fade_in_duration: fade,
                    fade_out_duration: fade,
                },
                duration,
            )
        });
        Ok(DJCommand::SetSignalPreset(self.tier, playback))
    }
}

/// Sets (or, with `status` unset, clears) the channel status of `tier`.
/// `duration` is in seconds and applies only when setting.
#[derive(Debug, Deserialize)]
pub struct ChannelStatusRequest {
    pub tier: ConfigurableTier,
    pub status: Option<ChannelStatus>,
    pub duration: Option<f32>,
}

impl ChannelStatusRequest {
    pub fn into_command(self) -> anyhow::Result<DJCommand> {
        let duration = optional_seconds(self.duration, "duration")?;
        let status = self.status.map(|status| TimedValue::new(status, duration));
        Ok(DJCommand::SetChannelStatus(self.tier, status))
    }
}

pub struct DJManager {
    state: Arc<RwLock<DJState>>,
    running: RwLock<Option<Running>>,
    guild_id: GuildId,
    bot_state: Data,
    audio_processor: ProcessorRef,
}

struct Running {
    task_handle: JoinHandle<()>,
    command_tx: mpsc::Sender<DJCommand>,
}

/// Marks a DJ command that was not sent because the DJ cannot take it right now.
#[derive(Debug, thiserror::Error)]
#[error("the DJ is busy")]
pub struct DjBusy;

impl DJManager {
    pub fn new(guild_id: GuildId, bot_state: Data, audio_processor: ProcessorRef) -> Self {
        Self {
            state: Arc::default(),
            running: RwLock::new(None),
            guild_id,
            bot_state,
            audio_processor,
        }
    }

    pub fn state(&self) -> Arc<RwLock<DJState>> {
        self.state.clone()
    }

    /// Sends `command` to the running task, returning false when none is running.
    /// Fails with [`DjBusy`] instead of waiting when the DJ is starting,
    /// stopping, or has a full command queue.
    pub fn send(&self, command: DJCommand) -> anyhow::Result<bool> {
        let running = self
            .running
            .try_read()
            .map_err(|_| DjBusy)
            .context("the DJ is starting or stopping")?;
        let Some(running) = running.as_ref() else {
            return Ok(false);
        };
        match running.command_tx.try_send(command) {
            Ok(()) => Ok(true),
            Err(TrySendError::Full(_)) => Err(DjBusy).context("the DJ command queue is full"),
            Err(TrySendError::Closed(_)) => bail!("the DJ task has exited"),
        }
    }

    fn spawn(
        &self,
        running: &mut Option<Running>,
        saved: Option<crate::persistence::DJState>,
    ) -> anyhow::Result<()> {
        let config_name = saved
            .as_ref()
            .and_then(|saved| saved.state_machine.as_ref())
            .map(|state| state.config_name.clone())
            .unwrap_or_else(|| DEFAULT_DJ_CONFIG.to_string());
        let config = self.bot_state.dj_config_manager.subscribe(&config_name)?;
        let (command_tx, command_rx) = mpsc::channel(10);
        let guild_id = self.guild_id;
        let task_state = self.state.clone();
        let bot_state = self.bot_state;
        let audio_processor = self.audio_processor.clone();
        let span = guild_span!(root "dj", guild_id);
        let task_handle = tokio::spawn(
            async move {
                DJStateMachine::new(
                    guild_id,
                    saved,
                    task_state,
                    config,
                    bot_state,
                    audio_processor,
                )
                .await
                .run(command_rx)
                .await;
            }
            .instrument(span),
        );
        *running = Some(Running {
            task_handle,
            command_tx,
        });
        Ok(())
    }

    #[tracing::instrument(skip_all, fields(guild_id = self.guild_id.get()))]
    pub async fn stop(&self) {
        let mut running = self.running.write().await;
        let Some(Running {
            mut task_handle,
            command_tx,
        }) = running.take()
        else {
            return;
        };
        if let Err(e) = command_tx
            .send_timeout(DJCommand::Stop, STOP_SEND_TIMEOUT)
            .await
        {
            tracing::warn!("Failed to send stop command to DJ task, aborting it: {e:#}");
            task_handle.abort();
            let _ = task_handle.await;
            return;
        }

        tracing::info!("Waiting for DJ graceful shutdown");
        tokio::select! {
            _ = &mut task_handle => {
                tracing::info!("DJ gracefully stopped");
            }
            _ = tokio::time::sleep(Duration::from_secs(5)) => {
                task_handle.abort();
                let _ = (&mut task_handle).await;
                tracing::warn!("DJ shutdown timeout, aborted the task");
            }
        }
    }

    /// Starts the DJ task if it is not running, resuming its saved state when
    /// there is one.
    #[tracing::instrument(skip_all, fields(guild_id = self.guild_id.get()))]
    pub async fn start(&self) -> anyhow::Result<()> {
        let mut running = self.running.write().await;
        if running.is_some() {
            return Ok(());
        }

        let saved = self
            .bot_state
            .state_store
            .load_dj_states()
            .await
            .context("loading saved DJ state")?
            .remove(&self.guild_id);

        tracing::info!("Starting DJ");
        self.spawn(&mut running, saved)
    }
}

pub const DEFAULT_DJ_CONFIG: &str = "default";
