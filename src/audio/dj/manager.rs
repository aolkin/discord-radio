use crate::audio::dj::config::{ConfiguredPool, DJConfig};
use crate::audio::dj::segments::SignalProfilePlayback;
use crate::audio::dj::state_machine::{DJState, DJStateMachine};
use crate::state::Data;
use anyhow::{Context, bail};
use serde::Deserialize;
use serenity::model::id::{ChannelId, GuildId};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc::error::TrySendError;
use tokio::sync::{RwLock, mpsc};
use tokio::task::JoinHandle;
use tracing::Instrument;

pub const ADVANCE_RETRY_DELAY: Duration = Duration::from_millis(100);
const STOP_SEND_TIMEOUT: Duration = Duration::from_secs(1);

#[derive(strum::IntoStaticStr)]
pub enum DJCommand {
    ForceAdvance(Option<String>),
    ForceHexMessage(HexMessageRequest),
    ForceTrack(TrackRequest),
    ForceIdle(IdleRequest),
    Stop,
    SetConfig {
        config: Arc<DJConfig>,
        pools: Arc<[ConfiguredPool]>,
    },
    SetAnnouncementChannel(Option<ChannelId>),
    SetBackgroundProfile(Option<SignalProfilePlayback>),
    SetOverrideProfile(Option<SignalProfilePlayback>),
}

/// A hex message for the DJ to play, with hex defaults from the pool named
/// `pool` when set. `volume` and `loops` replace the defaults when set.
pub struct HexMessageRequest {
    pub message: String,
    pub volume: Option<f32>,
    pub loops: Option<u32>,
    pub pool: Option<String>,
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
        for (name, value) in [
            ("start_position", self.start_position),
            ("duration", self.duration),
        ] {
            if let Some(seconds) = value
                && !(seconds.is_finite() && seconds >= 0.0)
            {
                bail!("{name} must be a non-negative number of seconds");
            }
        }
        Ok(())
    }
}

/// An idle segment for the DJ to enter, forcing `noise_profile` when set.
/// `duration` is in seconds; without it the segment never ends on its own.
#[derive(Debug, Deserialize)]
pub struct IdleRequest {
    pub noise_profile: Option<String>,
    pub duration: Option<f32>,
}

impl IdleRequest {
    pub fn validate(&self) -> anyhow::Result<()> {
        if let Some(seconds) = self.duration
            && !(seconds.is_finite() && seconds >= 0.0)
        {
            bail!("duration must be a non-negative number of seconds");
        }
        Ok(())
    }
}

pub struct DJManager {
    state: Arc<RwLock<DJState>>,
    running: RwLock<Option<Running>>,
    guild_id: GuildId,
    bot_state: Data,
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
    pub fn new(guild_id: GuildId, bot_state: Data) -> Self {
        Self {
            state: Arc::default(),
            running: RwLock::new(None),
            guild_id,
            bot_state,
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

    fn spawn(&self, running: &mut Option<Running>, saved: Option<crate::persistence::DJState>) {
        let (command_tx, command_rx) = mpsc::channel(10);
        let config_name = saved
            .as_ref()
            .and_then(|saved| saved.config_name.clone())
            .unwrap_or_else(|| DEFAULT_DJ_CONFIG.to_string());
        let (config, pools) = self
            .bot_state
            .dj_config_manager
            .get(&config_name)
            .unwrap_or_else(|e| {
                tracing::warn!(guild_id = %self.guild_id, "Starting DJ with an empty config: {e:#}");
                let empty = DJConfig {
                    name: config_name.clone(),
                    ..DJConfig::default()
                };
                (Arc::new(empty), Arc::default())
            });
        let guild_id = self.guild_id;
        let task_state = self.state.clone();
        let bot_state = self.bot_state;
        let span = tracing::info_span!("dj", guild_id = %guild_id);
        let task_handle = tokio::spawn(
            async move {
                DJStateMachine::new(guild_id, saved, task_state, config, pools, bot_state)
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
    }

    #[tracing::instrument(skip_all, fields(guild_id = %self.guild_id))]
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
            tracing::warn!("Failed to send stop command to DJ task, aborting it: {}", e);
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
    #[tracing::instrument(skip_all, fields(guild_id = %self.guild_id))]
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
        self.spawn(&mut running, saved);
        Ok(())
    }
}

pub const DEFAULT_DJ_CONFIG: &str = "default";
