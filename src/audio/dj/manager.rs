use crate::audio::dj::config::DJConfig;
use crate::audio::dj::profile_machine::ProfileStateMachine;
use crate::audio::dj::segments::SegmentCtx;
use crate::audio::dj::state_machine::{DJState, DJStateMachine};
use crate::state::Data;
use serde::Deserialize;
use serenity::all::Http;
use serenity::model::id::{ChannelId, GuildId};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::time::{Duration, sleep};

const DJ_TICK_INTERVAL_MS: u64 = 100;

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

pub async fn dj_task(
    guild_id: GuildId,
    config: DJConfig,
    bot_state: Data,
    mut command_rx: mpsc::Receiver<DJCommand>,
    announcement_channel: Option<ChannelId>,
    http: Arc<Http>,
    restored_state: Option<DJState>,
) {
    let config_name = config.name.clone();

    tracing::info!(
        "Starting DJ task for guild {} with config '{}'",
        guild_id,
        config_name
    );

    // Clone signal_profiles before moving config
    let signal_profiles = config.signal_profiles.clone();

    let initial_state = if let Some(state) = restored_state {
        state
    } else {
        DJState::idle()
    };

    let (forced_profile, status_message) = {
        let segment = &initial_state.segment;
        let _ = segment
            .restore(&SegmentCtx::new(&guild_id, bot_state))
            .await
            .inspect_err(|e| tracing::warn!("Failed to restore segment: {e:?}"));
        (segment.signal_profile().name, segment.channel_status())
    };

    let dj_state = Arc::new(tokio::sync::RwLock::new(initial_state));
    bot_state
        .dj_states
        .write()
        .await
        .insert(guild_id, dj_state.clone());

    let mut state_machine = DJStateMachine::new(
        config.clone(),
        guild_id,
        announcement_channel,
        http.clone(),
        dj_state,
        bot_state,
    );

    // Initialize profile state machine
    let mut profile_machine = if !signal_profiles.is_empty() {
        let forced_profile_name = forced_profile.as_deref();

        // If no forced profile, try to restore the last active profile from ProfileState
        let initial_profile_name = if let Some(profile_name) = forced_profile_name {
            Some(profile_name.to_string())
        } else if let Ok(profile_states) = bot_state.state_store.load_profile_states().await {
            profile_states
                .get(&guild_id)
                .filter(|ps| !ps.bypass)
                .map(|ps| ps.profile_name.clone())
        } else {
            None
        };

        Some(ProfileStateMachine::new(
            signal_profiles,
            initial_profile_name.as_deref(),
        ))
    } else {
        None
    };

    if let Some(status_msg) = status_message {
        bot_state
            .voice_status_manager
            .push_status(guild_id, &status_msg, &http)
            .await;
        tracing::info!(
            "Restored channel status '{}' in guild {}",
            status_msg,
            guild_id
        );
    }

    'dj: loop {
        sleep(Duration::from_millis(DJ_TICK_INTERVAL_MS)).await;

        // Process any pending commands
        while let Ok(cmd) = command_rx.try_recv() {
            if state_machine.handle_command(cmd).await {
                tracing::info!("DJ task stopped for guild {}", guild_id);
                break 'dj;
            }
        }

        if state_machine.is_complete().await
            && let Err(e) = state_machine.force_advance(&bot_state, None).await
        {
            tracing::error!("DJ state machine error in guild {}: {}", guild_id, e);
        }

        // Persist DJ state periodically (including state machine state)
        persist_state(guild_id, bot_state, &config_name, &mut state_machine).await;

        if let Some(ref mut pm) = profile_machine {
            let forced_profile = state_machine.current_state().await.forced_profile();
            if let Some((profile_name, fade_secs, reason)) = pm.update(forced_profile) {
                apply_profile_transition(
                    guild_id,
                    Some(&profile_name),
                    fade_secs,
                    &bot_state,
                    reason,
                )
                .await;
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

async fn apply_profile_transition(
    guild_id: GuildId,
    profile_name: Option<&str>,
    fade_secs: f32,
    bot_state: &Data,
    reason: &str,
) {
    if let Some(profile_name) = profile_name {
        let processors = bot_state.audio_processors.read().await;
        if let Some(processor_arc) = processors.get(&guild_id)
            && let Some(new_profile) = bot_state.profile_manager.get_profile(profile_name)
        {
            let mut processor = processor_arc.write().await;
            processor.start_profile_transition(new_profile.clone(), fade_secs * 1000.0);

            tracing::info!(
                "DJ transitioning to profile '{}' over {:.1}s {} in guild {}",
                profile_name,
                fade_secs,
                reason,
                guild_id
            );
        }

        // Persist the profile state for restoration on restart
        let profile_state = crate::persistence::ProfileState {
            profile_name: profile_name.to_string(),
            bypass: false,
        };
        if let Err(e) = bot_state
            .state_store
            .save_profile_state(guild_id, &profile_state)
            .await
        {
            tracing::warn!("Failed to save profile state for guild {}: {}", guild_id, e);
        }
    }
}

async fn persist_state(
    guild_id: GuildId,
    bot_state: Data,
    config_name: &String,
    state_machine: &mut DJStateMachine,
) {
    let current_state = state_machine.current_state().await;
    let persist_state = crate::persistence::DJStateSnapshot {
        config_name,
        running: true,
        announcement_channel_id: state_machine.announcement_channel().map(|id| id.get()),
        state_machine: &current_state,
    };
    if let Err(e) = bot_state
        .state_store
        .save_dj_state(guild_id, &persist_state)
        .await
    {
        tracing::warn!("Failed to persist DJ state for guild {}: {}", guild_id, e);
    }
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
            dj_task(
                guild_id,
                config,
                bot_state,
                rx,
                announcement_channel,
                http_clone,
                restored_state,
            )
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
