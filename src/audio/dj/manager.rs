use crate::audio::dj::config::DJConfig;
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
    status_message: Option<String>,
}

impl DJManager {
    pub fn new(guild_id: GuildId) -> Self {
        Self {
            task_handle: None,
            command_tx: None,
            guild_id,
            status_message: None,
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

        // Set voice channel status if configured using the status stack
        if let Some(status) = &config.channel_status {
            bot_state
                .voice_status_manager
                .push_status(self.guild_id, status, &http)
                .await;
            tracing::info!(
                "Pushed DJ voice channel status '{}' for guild {}",
                status,
                self.guild_id
            );
            // Store the status message so we can remove it later
            self.status_message = Some(status.clone());
        }

        let config_name = config.name.clone();
        let (tx, rx) = mpsc::channel(10);
        let guild_id = self.guild_id;
        let http_clone = http.clone();
        let handle = tokio::spawn(async move {
            DJStateMachine::new(
                config,
                guild_id,
                announcement_channel,
                http_clone,
                restored_state,
                bot_state,
            )
            .await
            .run(rx)
            .await;
        });

        self.task_handle = Some(handle);
        self.command_tx = Some(tx);

        // Save DJ state to persistence (initial state will be Idle)
        let idle = DJState::idle();
        let dj_state = crate::persistence::DJStateSnapshot {
            config_name: &config_name,
            running: true,
            announcement_channel_id: announcement_channel.map(|id| id.get()),
            state_machine: &idle,
        };
        if let Err(e) = bot_state
            .state_store
            .save_dj_state(guild_id, &dj_state)
            .await
        {
            tracing::warn!("Failed to save DJ state for guild {}: {}", guild_id, e);
        }

        Ok(())
    }

    pub async fn stop(&mut self, bot_state: &Data, http: Arc<Http>) {
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

            // Remove the DJ's voice channel status from the stack
            if let Some(status_msg) = &self.status_message {
                bot_state
                    .voice_status_manager
                    .remove_status(self.guild_id, status_msg, &http)
                    .await;
                tracing::info!(
                    "Removed DJ voice channel status for guild {}",
                    self.guild_id
                );
            }
            self.status_message = None;

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
