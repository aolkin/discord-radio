use crate::audio::dj::config::DJConfig;
use crate::audio::dj::segments::SignalProfilePlayback;
use crate::audio::dj::state_machine::{DJState, DJStateMachine};
use crate::state::Data;
use anyhow::{Context, bail};
use serde::Deserialize;
use serenity::model::id::{ChannelId, GuildId};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;
use tokio::sync::{Mutex, RwLock, mpsc};
use tokio::task::JoinHandle;

pub const ADVANCE_RETRY_DELAY: Duration = Duration::from_millis(100);

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
    /// `command_tx` changes only while this is locked, and is `Some` exactly
    /// when this is.
    task: Mutex<Option<JoinHandle<()>>>,
    command_tx: StdMutex<Option<mpsc::Sender<DJCommand>>>,
    guild_id: GuildId,
    bot_state: Data,
}

impl DJManager {
    pub fn new(guild_id: GuildId, bot_state: Data) -> Self {
        Self {
            state: Arc::default(),
            task: Mutex::new(None),
            command_tx: StdMutex::new(None),
            guild_id,
            bot_state,
        }
    }

    /// The DJ's state while its task is running.
    pub fn state(&self) -> Option<Arc<RwLock<DJState>>> {
        let running = self.command_tx.lock().unwrap().is_some();
        running.then(|| self.state.clone())
    }

    /// Sends `command` to the running task, returning false when none is running.
    pub async fn send(&self, command: DJCommand) -> anyhow::Result<bool> {
        let Some(command_tx) = self.command_tx.lock().unwrap().clone() else {
            return Ok(false);
        };
        command_tx.send(command).await?;
        Ok(true)
    }

    fn spawn(&self, task: &mut Option<JoinHandle<()>>, saved: Option<crate::persistence::DJState>) {
        let (command_tx, command_rx) = mpsc::channel(10);
        let guild_id = self.guild_id;
        let task_state = self.state.clone();
        let bot_state = self.bot_state;
        *task = Some(tokio::spawn(async move {
            DJStateMachine::new(guild_id, saved, task_state, bot_state)
                .await
                .run(command_rx)
                .await;
        }));
        *self.command_tx.lock().unwrap() = Some(command_tx);
    }

    async fn stop_task(&self, task: &mut Option<JoinHandle<()>>) {
        let command_tx = self.command_tx.lock().unwrap().take();
        let Some(mut task_handle) = task.take() else {
            return;
        };
        if let Some(command_tx) = command_tx
            && let Err(e) = command_tx.send(DJCommand::Stop).await
        {
            tracing::warn!("Failed to send stop command to DJ task: {}", e);
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

        if let Err(e) = self
            .bot_state
            .state_store
            .remove_dj_state(self.guild_id)
            .await
        {
            tracing::warn!(
                "Failed to remove DJ state for guild {}: {}",
                self.guild_id,
                e
            );
        }
    }

    /// Starts the DJ task if it is not running, resuming its saved state when
    /// there is one.
    pub async fn start(&self) -> anyhow::Result<()> {
        let mut task = self.task.lock().await;
        if task.is_some() {
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
        self.spawn(&mut task, saved);
        Ok(())
    }

    /// Stops the running DJ task and starts a new one with the named config,
    /// keeping the announcement channel. The new task begins idle.
    pub async fn restart_with_config(&self, config_name: String) -> anyhow::Result<()> {
        let mut task = self.task.lock().await;
        if task.is_none() {
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

        self.stop_task(&mut task).await;

        let saved = crate::persistence::DJState {
            config_name: Some(config_name),
            announcement_channel_id,
            state_machine: None,
        };
        tracing::info!("Restarting DJ for guild {}", self.guild_id);
        self.spawn(&mut task, Some(saved));
        Ok(())
    }

    pub async fn stop(&self) {
        let mut task = self.task.lock().await;
        self.stop_task(&mut task).await;
    }
}

pub const DEFAULT_DJ_CONFIG: &str = "default";

pub fn load_dj_config(bot_state: &Data, name: &str) -> anyhow::Result<Arc<DJConfig>> {
    bot_state
        .dj_config_manager
        .get(name)
        .with_context(|| format!("no DJ config {name:?}"))
}

pub async fn start_dj(bot_state: Data, guild_id: GuildId) -> anyhow::Result<()> {
    bot_state.dj_manager(guild_id).start().await
}

pub async fn restart_dj_with_config(
    bot_state: Data,
    guild_id: GuildId,
    config_name: String,
) -> anyhow::Result<()> {
    load_dj_config(&bot_state, &config_name)?;
    let Some(manager) = bot_state.existing_dj_manager(guild_id) else {
        bail!("Not in a voice channel");
    };
    manager.restart_with_config(config_name).await
}

pub async fn stop_dj(bot_state: Data, guild_id: GuildId) {
    if let Some(manager) = bot_state.existing_dj_manager(guild_id) {
        manager.stop().await;
    }
}
