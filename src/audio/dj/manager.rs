use crate::audio::dj::config::DJConfig;
use crate::audio::dj::state_machine::{DJState, DJStateMachine};
use crate::state::Data;
use serenity::all::Http;
use serenity::model::id::{ChannelId, GuildId};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::time::{Duration, sleep};

const DJ_TICK_INTERVAL_MS: u64 = 100;

pub enum DJCommand {
    ForceAdvance,
    Stop,
}

pub async fn dj_task(
    guild_id: GuildId,
    config: DJConfig,
    bot_state: Data,
    mut command_rx: mpsc::Receiver<DJCommand>,
) {
    tracing::info!(
        "Starting DJ task for guild {} with config '{}'",
        guild_id,
        config.name
    );

    let mut state_machine = DJStateMachine::new(config, guild_id, bot_state.hex_audio_dir.clone());

    // Create shared state for this DJ
    let state_arc = {
        let mut dj_states = bot_state.dj_states.write().await;

        dj_states
            .entry(guild_id)
            .or_insert_with(|| {
                std::sync::Arc::new(tokio::sync::RwLock::new(
                    state_machine.current_state().clone(),
                ))
            })
            .clone()
    };

    let mut pending_stop = false;

    loop {
        tokio::select! {
            _ = sleep(Duration::from_millis(DJ_TICK_INTERVAL_MS)) => {},
            Some(cmd) = command_rx.recv() => {
                match cmd {
                    DJCommand::ForceAdvance => {
                        tracing::info!("Force advancing DJ state for guild {}", guild_id);
                        state_machine.force_complete();
                    }
                    DJCommand::Stop => {
                        tracing::info!("Received stop command for DJ in guild {}", guild_id);
                        pending_stop = true;
                    }
                }
            }
        }

        let track_managers = bot_state.track_managers.read().await;
        let manager_arc = match track_managers.get(&guild_id) {
            Some(arc) => arc.clone(),
            None => {
                tracing::warn!(
                    "No track manager found for guild {} in DJ task, waiting...",
                    guild_id
                );
                drop(track_managers);
                sleep(Duration::from_secs(1)).await;
                continue;
            }
        };
        drop(track_managers);

        let mut manager = manager_arc.lock().await;

        // Handle pending stop command now that we have the track manager
        if pending_stop {
            tracing::info!("Processing stop command for DJ in guild {}", guild_id);
            state_machine.stop(&mut manager).await;
            pending_stop = false;
        }

        // Check for stop state
        if matches!(state_machine.current_state(), DJState::Stopped) {
            drop(manager);
            tracing::info!("DJ task stopped for guild {}", guild_id);
            break;
        }

        if let Err(e) = state_machine.advance(&mut manager, &bot_state).await {
            tracing::error!("DJ state machine error in guild {}: {}", guild_id, e);
        }

        // Update shared state
        {
            let mut state = state_arc.write().await;
            *state = state_machine.current_state().clone();
        }

        let current_state = state_machine.current_state();

        if let DJState::PlayingHexMessage { message, .. } = current_state {
            let hex_playback_states = bot_state.hex_playback_states.read().await;
            if let Some(state_arc) = hex_playback_states.get(&guild_id) {
                let state = state_arc.read().await;
                if state.message.is_none() || state.message.as_ref() != Some(message) {
                    drop(state);
                    drop(hex_playback_states);

                    if let Err(e) = state_machine.advance(&mut manager, &bot_state).await {
                        tracing::error!("DJ failed to advance after hex message completion: {}", e);
                    }
                }
            }
        }
    }

    // Clean up shared state
    {
        let mut dj_states = bot_state.dj_states.write().await;
        dj_states.remove(&guild_id);
    }

    tracing::info!("DJ task terminated for guild {}", guild_id);
}

pub struct DJManager {
    task_handle: Option<tokio::task::JoinHandle<()>>,
    command_tx: Option<mpsc::Sender<DJCommand>>,
    guild_id: GuildId,
    channel_id: Option<ChannelId>,
}

impl DJManager {
    pub fn new(guild_id: GuildId) -> Self {
        Self {
            task_handle: None,
            command_tx: None,
            guild_id,
            channel_id: None,
        }
    }

    pub async fn start(
        &mut self,
        config: DJConfig,
        bot_state: Data,
        http: Arc<Http>,
        channel_id: ChannelId,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if self.task_handle.is_some() {
            return Err("DJ is already running".into());
        }

        // Set voice channel status if configured
        if let Some(status) = &config.channel_status {
            if let Err(e) = channel_id
                .edit(&http, serenity::all::EditChannel::new().status(status))
                .await
            {
                tracing::warn!(
                    "Failed to set voice channel status for guild {}: {}",
                    self.guild_id,
                    e
                );
            } else {
                tracing::info!(
                    "Set voice channel status to '{}' for guild {}",
                    status,
                    self.guild_id
                );
            }
        }

        self.channel_id = Some(channel_id);

        let config_name = config.name.clone();
        let (tx, rx) = mpsc::channel(10);
        let guild_id = self.guild_id;
        let bot_state_clone = bot_state.clone();
        let handle = tokio::spawn(async move {
            dj_task(guild_id, config, bot_state_clone, rx).await;
        });

        self.task_handle = Some(handle);
        self.command_tx = Some(tx);

        // Save DJ state to persistence
        let dj_state = crate::persistence::DJState {
            config_name,
            running: true,
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
            if let Some(tx) = &self.command_tx
                && let Err(e) = tx.send(DJCommand::Stop).await
            {
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

            // Clear voice channel status
            if let Some(channel_id) = self.channel_id {
                if let Err(e) = channel_id
                    .edit(&http, serenity::all::EditChannel::new().status(""))
                    .await
                {
                    tracing::warn!(
                        "Failed to clear voice channel status for guild {}: {}",
                        self.guild_id,
                        e
                    );
                } else {
                    tracing::info!("Cleared voice channel status for guild {}", self.guild_id);
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

    pub async fn force_advance(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if let Some(tx) = &self.command_tx {
            tx.send(DJCommand::ForceAdvance).await?;
            Ok(())
        } else {
            Err("DJ is not running".into())
        }
    }
}

pub async fn force_advance(
    bot_state: &Data,
    guild_id: GuildId,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let dj_managers = bot_state.dj_managers.read().await;
    let manager = dj_managers
        .get(&guild_id)
        .ok_or("DJ manager not found")?
        .clone();
    drop(dj_managers);

    let mgr = manager.lock().await;
    mgr.force_advance().await
}
