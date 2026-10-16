use crate::audio::dj::config::DJConfig;
use crate::audio::dj::segments::SignalProfilePlayback;
use crate::audio::dj::state_machine::{DJState, DJStateMachine};
use crate::state::Data;
use serde::Deserialize;
use serenity::all::Http;
use serenity::model::id::{ChannelId, GuildId};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

pub const ADVANCE_RETRY_DELAY: Duration = Duration::from_millis(100);

#[derive(strum::IntoStaticStr)]
pub enum DJCommand {
    ForceAdvance(Option<DJStateType>),
    ForceHexMessage(HexMessageRequest),
    ForceTrack(TrackRequest),
    Stop,
    SetAnnouncementChannel(Option<ChannelId>),
    SetBackgroundProfile(Option<SignalProfilePlayback>),
    SetOverrideProfile(Option<SignalProfilePlayback>),
    SetChannelStatusEnabled(bool),
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
    pub fn validate(&self) -> Result<(), String> {
        for (name, value) in [
            ("start_position", self.start_position),
            ("duration", self.duration),
        ] {
            if let Some(seconds) = value
                && !(seconds.is_finite() && seconds >= 0.0)
            {
                return Err(format!("{name} must be a non-negative number of seconds"));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug)]
pub enum DJStateType {
    Track,
    HexMessage,
    Noise,
}

pub struct DJManager {
    task_handle: Option<tokio::task::JoinHandle<()>>,
    pub(crate) command_tx: Option<mpsc::Sender<DJCommand>>,
    guild_id: GuildId,
}

impl DJManager {
    pub fn new(guild_id: GuildId) -> Self {
        Self {
            task_handle: None,
            command_tx: None,
            guild_id,
        }
    }

    pub async fn send(
        &self,
        command: DJCommand,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if let Some(tx) = &self.command_tx {
            tx.send(command).await?;
            Ok(())
        } else {
            Err("DJ is not running".into())
        }
    }

    pub async fn start(
        &mut self,
        config: DJConfig,
        bot_state: Data,
        http: Arc<Http>,
        announcement_channel: Option<ChannelId>,
        restored_state: Option<DJState>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if self.task_handle.is_some() {
            return Err("DJ is already running".into());
        }

        let (tx, rx) = mpsc::channel(10);
        let guild_id = self.guild_id;
        let handle = tokio::spawn(async move {
            DJStateMachine::new(
                config,
                guild_id,
                announcement_channel,
                http,
                restored_state,
                bot_state,
            )
            .await
            .run(rx)
            .await;
        });

        self.task_handle = Some(handle);
        self.command_tx = Some(tx);

        Ok(())
    }

    pub async fn stop(&mut self, bot_state: &Data) {
        if let Some(handle) = self.task_handle.take() {
            // Send graceful stop command
            if let Err(e) = self.send(DJCommand::Stop).await {
                tracing::warn!("Failed to send stop command to DJ task: {}", e);
            }
            self.command_tx = None;

            // Wait for graceful shutdown with timeout
            tracing::info!(
                "Waiting for DJ graceful shutdown for guild {}",
                self.guild_id
            );
            tokio::select! {
                _ = handle => {
                    tracing::info!("DJ gracefully stopped for guild {}", self.guild_id);
                }
                _ = tokio::time::sleep(Duration::from_secs(5)) => {
                    tracing::warn!("DJ shutdown timeout for guild {}, task may have hung", self.guild_id);
                }
            }

            // Remove DJ state from persistence
            if let Err(e) = bot_state.state_store.remove_dj_state(self.guild_id).await {
                tracing::warn!(
                    "Failed to remove DJ state for guild {}: {}",
                    self.guild_id,
                    e
                );
            }
        }
    }

    pub fn is_running(&self) -> bool {
        self.task_handle.is_some()
    }
}

pub async fn send_dj_command(
    bot_state: &Data,
    guild_id: GuildId,
    command: DJCommand,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let manager = bot_state.dj_manager(guild_id).await;
    let mgr = manager.lock().await;
    mgr.send(command).await
}

/// Save whether the DJ manages the guild's voice channel status, and apply it
/// to the running DJ.
pub async fn set_channel_status_enabled(
    bot_state: &Data,
    guild_id: GuildId,
    enabled: bool,
) -> anyhow::Result<()> {
    bot_state
        .dj_settings
        .update(guild_id, |settings| {
            settings.channel_status_disabled = !enabled;
            Ok(())
        })
        .await
        .map_err(|e| anyhow::anyhow!("Failed to save channel status setting: {e}"))?;

    let manager = bot_state.dj_manager(guild_id).await;
    let mgr = manager.lock().await;
    if mgr.is_running() {
        mgr.send(DJCommand::SetChannelStatusEnabled(enabled))
            .await
            .map_err(|e| anyhow::anyhow!("Failed to notify the DJ: {e}"))?;
    }
    Ok(())
}
