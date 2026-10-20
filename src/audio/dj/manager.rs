use crate::audio::dj::config::DJConfig;
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

pub const ADVANCE_RETRY_DELAY: Duration = Duration::from_millis(100);
const STOP_SEND_TIMEOUT: Duration = Duration::from_secs(1);

#[derive(strum::IntoStaticStr)]
pub enum DJCommand {
    ForceAdvance(Option<DJStateType>),
    ForceHexMessage(HexMessageRequest),
    ForceTrack(TrackRequest),
    ForceIdle(IdleRequest),
    Stop,
    SetAnnouncementChannel(Option<ChannelId>),
    SetBackgroundProfile(Option<SignalProfilePlayback>),
    SetOverrideProfile(Option<SignalProfilePlayback>),
}

/// A hex message for the DJ to play. `volume` and `loops` replace the DJ's
/// defaults when set.
pub struct HexMessageRequest {
    pub message: String,
    pub volume: Option<f32>,
    pub loops: Option<u32>,
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

#[derive(Clone, Copy, Debug, poise::ChoiceParameter)]
pub enum DJStateType {
    Track,
    #[name = "hex"]
    HexMessage,
    Noise,
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
        let guild_id = self.guild_id;
        let task_state = self.state.clone();
        let bot_state = self.bot_state;
        let task_handle = tokio::spawn(async move {
            DJStateMachine::new(guild_id, saved, task_state, bot_state)
                .await
                .run(command_rx)
                .await;
        });
        *running = Some(Running {
            task_handle,
            command_tx,
        });
    }

    async fn stop_task(&self, running: &mut Option<Running>) {
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
            tracing::warn!(
                "Failed to send stop command to DJ task for guild {}, aborting it: {}",
                self.guild_id,
                e
            );
            task_handle.abort();
            let _ = task_handle.await;
            return;
        }

        tracing::info!(
            "Waiting for DJ graceful shutdown for guild {}",
            self.guild_id
        );
        tokio::select! {
            _ = &mut task_handle => {
                tracing::info!("DJ gracefully stopped for guild {}", self.guild_id);
            }
            _ = tokio::time::sleep(Duration::from_secs(5)) => {
                task_handle.abort();
                let _ = (&mut task_handle).await;
                tracing::warn!("DJ shutdown timeout for guild {}, aborted the task", self.guild_id);
            }
        }
    }

    /// Starts the DJ task if it is not running, resuming its saved state when
    /// there is one.
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

        tracing::info!("Starting DJ for guild {}", self.guild_id);
        self.spawn(&mut running, saved);
        Ok(())
    }

    /// Stops the running DJ task and starts a new one with the named config,
    /// keeping the announcement channel. The new task begins idle.
    pub async fn restart_with_config(&self, config_name: String) -> anyhow::Result<()> {
        load_dj_config(&self.bot_state, &config_name)?;
        let mut running = self.running.write().await;
        if running.is_none() {
            bail!("Not in a voice channel");
        }

        let announcement_channel_id = self
            .bot_state
            .state_store
            .load_dj_states()
            .await
            .context("loading saved DJ state")?
            .remove(&self.guild_id)
            .and_then(|saved| saved.announcement_channel_id);

        self.stop_task(&mut running).await;

        let saved = crate::persistence::DJState {
            config_name: Some(config_name),
            announcement_channel_id,
            state_machine: None,
        };
        tracing::info!("Restarting DJ for guild {}", self.guild_id);
        self.spawn(&mut running, Some(saved));
        Ok(())
    }

    pub async fn stop(&self) {
        let mut running = self.running.write().await;
        self.stop_task(&mut running).await;
    }
}

pub const DEFAULT_DJ_CONFIG: &str = "default";

pub fn load_dj_config(bot_state: &Data, name: &str) -> anyhow::Result<Arc<DJConfig>> {
    bot_state
        .dj_config_manager
        .get(name)
        .with_context(|| format!("no DJ config {name:?}"))
}
