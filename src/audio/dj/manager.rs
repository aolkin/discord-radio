use crate::audio::dj::config::DJConfig;
use crate::audio::dj::segments::SignalProfilePlayback;
use crate::audio::dj::state_machine::{DJState, DJStateMachine};
use crate::state::Data;
use anyhow::{Context, bail};
use serde::Deserialize;
use serenity::model::id::{ChannelId, GuildId};
use std::sync::Arc;
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
    pub state: Arc<RwLock<DJState>>,
    task_handle: JoinHandle<()>,
    command_tx: mpsc::Sender<DJCommand>,
    guild_id: GuildId,
}

impl DJManager {
    pub async fn send(&self, command: DJCommand) -> anyhow::Result<()> {
        Ok(self.command_tx.send(command).await?)
    }

    pub async fn start(
        guild_id: GuildId,
        config: DJConfig,
        saved: Option<crate::persistence::DJState>,
        bot_state: Data,
    ) -> Self {
        let (command_tx, command_rx) = mpsc::channel(10);
        let state = Arc::new(RwLock::new(DJState::default()));
        let task_state = state.clone();
        let task_handle = tokio::spawn(async move {
            DJStateMachine::new(config, guild_id, saved, task_state, bot_state)
                .await
                .run(command_rx)
                .await;
        });

        Self {
            state,
            task_handle,
            command_tx,
            guild_id,
        }
    }

    pub async fn stop(&mut self, bot_state: &Data) {
        if let Err(e) = self.send(DJCommand::Stop).await {
            tracing::warn!("Failed to send stop command to DJ task: {}", e);
        }

        tracing::info!(
            "Waiting for DJ graceful shutdown for guild {}",
            self.guild_id
        );
        tokio::select! {
            _ = &mut self.task_handle => {
                tracing::info!("DJ gracefully stopped for guild {}", self.guild_id);
            }
            _ = tokio::time::sleep(Duration::from_secs(5)) => {
                self.task_handle.abort();
                let _ = (&mut self.task_handle).await;
                tracing::warn!("DJ shutdown timeout for guild {}, aborted the task", self.guild_id);
            }
        }

        if let Err(e) = bot_state.state_store.remove_dj_state(self.guild_id).await {
            tracing::warn!(
                "Failed to remove DJ state for guild {}: {}",
                self.guild_id,
                e
            );
        }
    }
}

pub async fn load_dj_config(
    bot_state: &Data,
    guild_id: GuildId,
    config_name: Option<&str>,
) -> anyhow::Result<DJConfig> {
    let base = match config_name {
        Some(name) => DJConfig::load_from_file(&bot_state.dj_config_path(name))
            .with_context(|| format!("loading DJ config '{name}'"))?,
        None => DJConfig::default(),
    };
    let settings = bot_state.dj_settings.get(guild_id).await;
    let mut config = base.with_overrides(&settings);
    config
        .apply_dj_settings(&settings, &bot_state.file_resolver)
        .await?;
    Ok(config)
}

/// Starts the guild's DJ if it is not running, resuming its saved state when
/// there is one.
pub async fn start_dj(bot_state: Data, guild_id: GuildId) -> anyhow::Result<()> {
    let mut dj_managers = bot_state.dj_managers.write().await;
    if dj_managers.contains_key(&guild_id) {
        return Ok(());
    }

    let saved = bot_state
        .state_store
        .load_dj_states()
        .await
        .context("loading saved DJ state")?
        .remove(&guild_id);

    let config_name = saved.as_ref().and_then(|s| s.config_name.as_deref());
    let config = load_dj_config(&bot_state, guild_id, config_name)
        .await
        .unwrap_or_else(|e| {
            tracing::warn!("Starting DJ with an empty config in guild {guild_id}: {e:#}");
            DJConfig::default()
        });

    tracing::info!("Starting DJ for guild {}", guild_id);
    let manager = DJManager::start(guild_id, config, saved, bot_state).await;
    dj_managers.insert(guild_id, Arc::new(Mutex::new(manager)));
    Ok(())
}

/// Stops the guild's running DJ and starts a new one with the named config,
/// keeping the announcement channel. The new DJ begins idle.
pub async fn restart_dj_with_config(
    bot_state: Data,
    guild_id: GuildId,
    config_name: String,
) -> anyhow::Result<()> {
    let config = load_dj_config(&bot_state, guild_id, Some(&config_name)).await?;

    let mut dj_managers = bot_state.dj_managers.write().await;
    let Some(manager) = dj_managers.remove(&guild_id) else {
        anyhow::bail!("Not in a voice channel");
    };

    let announcement_channel_id = bot_state
        .state_store
        .load_dj_states()
        .await
        .context("loading saved DJ state")?
        .remove(&guild_id)
        .and_then(|saved| saved.announcement_channel_id);

    manager.lock().await.stop(&bot_state).await;

    let saved = crate::persistence::DJState {
        config_name: Some(config_name),
        announcement_channel_id,
        state_machine: None,
    };
    tracing::info!("Restarting DJ for guild {}", guild_id);
    let manager = DJManager::start(guild_id, config, Some(saved), bot_state).await;
    dj_managers.insert(guild_id, Arc::new(Mutex::new(manager)));
    Ok(())
}

pub async fn stop_dj(bot_state: Data, guild_id: GuildId) {
    let manager = bot_state.dj_managers.write().await.remove(&guild_id);
    if let Some(manager) = manager {
        manager.lock().await.stop(&bot_state).await;
    }
}
