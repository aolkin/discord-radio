use crate::audio::dj::config::DJConfig;
use crate::audio::dj::profile_machine::ProfileStateMachine;
use crate::audio::dj::state_machine::{DJState, DJStateMachine};
use crate::audio::tracks::StartTrackArgs;
use crate::state::Data;
use serenity::all::Http;
use serenity::model::id::{ChannelId, GuildId};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::time::{Duration, sleep};

const DJ_TICK_INTERVAL_MS: u64 = 100;

pub enum DJCommand {
    ForceAdvance(Option<DJStateTypeFilter>),
    ForceHexMessage(String),
    ReloadConfig,
    Stop,
    SetAnnouncementChannel(Option<ChannelId>),
}

#[derive(Clone, Copy, Debug)]
pub enum DJStateTypeFilter {
    Track,
    HexMessage,
    Noise,
}

/// Helper function to find a track entry by matching filename in the track pool
fn find_track_by_filename<'a>(
    state_machine: &'a DJStateMachine,
    filename: &str,
) -> Option<&'a crate::audio::dj::config::TrackEntry> {
    // Search through the track pool for a matching filename
    let track_pool = &state_machine.scheduler().config().track_pool;

    let result = track_pool.iter().find(|entry| entry.filename == filename);

    if result.is_none() {
        tracing::warn!("Could not find filename '{}' in DJ config", filename);
    }

    result
}

pub async fn dj_task(
    guild_id: GuildId,
    config: DJConfig,
    bot_state: Data,
    mut command_rx: mpsc::Receiver<DJCommand>,
    mut announcement_channel: Option<ChannelId>,
    http: Arc<Http>,
    restored_state: Option<crate::persistence::DJStateMachineState>,
) {
    let config_name = config.name.clone();

    tracing::info!(
        "Starting DJ task for guild {} with config '{}'",
        guild_id,
        config_name
    );

    // Clone signal_profiles before moving config
    let signal_profiles = config.signal_profiles.clone();

    let mut state_machine = DJStateMachine::new(
        config.clone(),
        guild_id,
        bot_state.content_path.clone(),
        announcement_channel,
        http.clone(),
        restored_state.clone(),
    );

    // Initialize profile state machine
    let mut profile_machine = if !signal_profiles.is_empty() {
        // Check if the restored DJ state has a forced profile
        let forced_profile_name = restored_state.as_ref().and_then(|state| match state {
            crate::persistence::DJStateMachineState::PlayingTrack { forced_profile, .. } => {
                forced_profile.as_ref()
            }
            crate::persistence::DJStateMachineState::PlayingHexMessage {
                forced_profile, ..
            } => forced_profile.as_ref(),
            crate::persistence::DJStateMachineState::PlayingNoise { noise_profile, .. } => {
                Some(noise_profile)
            }
            _ => None,
        });

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

        let mut machine =
            ProfileStateMachine::new(signal_profiles, initial_profile_name.as_deref());

        // If the DJ state had a forced profile, set the machine to ForcedProfile state
        if let Some(profile_name) = forced_profile_name {
            machine.force_profile(profile_name.clone());
            tracing::info!(
                "Restored forced profile '{}' for DJ in guild {}",
                profile_name,
                guild_id
            );
        }

        Some(machine)
    } else {
        None
    };

    // If we restored a PlayingTrack state, the DJ should restart the track itself
    // since DJ tracks are now played in non-persisted mode
    if let Some(state) = restored_state {
        match state {
            crate::persistence::DJStateMachineState::PlayingTrack {
                track_name,
                filename,
                started_at,
                duration_secs,
                status_message,
                ..
            } => {
                tracing::debug!(
                    "Attempting to restore DJ track '{}' (file: {}) in guild {}",
                    track_name,
                    filename,
                    guild_id
                );

                // Wait for track manager to be available
                let manager_arc = loop {
                    let track_managers = bot_state.track_managers.read().await;
                    if let Some(arc) = track_managers.get(&guild_id) {
                        let arc_clone = arc.clone();
                        drop(track_managers);
                        break arc_clone;
                    }
                    drop(track_managers);
                    tracing::debug!(
                        "Waiting for track manager for guild {} during DJ restoration",
                        guild_id
                    );
                    sleep(Duration::from_millis(100)).await;
                };

                // Calculate how much time has elapsed since the track started
                let elapsed = match started_at.elapsed() {
                    Ok(elapsed) => elapsed,
                    Err(e) => {
                        tracing::warn!(
                            "Failed to calculate elapsed time for DJ track '{}': {}",
                            track_name,
                            e
                        );
                        Duration::from_secs(0)
                    }
                };

                let total_duration = Duration::from_secs_f32(duration_secs);

                // Only attempt to restart the track if it hasn't finished yet
                if elapsed < total_duration {
                    let mut manager = manager_arc.lock().await;

                    // Construct the full path - the filename in the persisted state is relative
                    let full_path = format!("{}/{}", bot_state.content_path, filename);

                    // Try to determine the original volume from the DJ's track pool by filename
                    let volume = find_track_by_filename(&state_machine, &filename)
                        .and_then(|entry| entry.volume)
                        .unwrap_or(1.0);

                    // Try to restart the track at the appropriate position
                    if let Err(e) = manager
                        .start_track(StartTrackArgs {
                            name: track_name.clone(),
                            filename: full_path,
                            volume,
                            fade_time: 1.0,
                            loops: false,
                            start_position: Some(elapsed),
                            persist: false,
                        })
                        .await
                    {
                        tracing::warn!(
                            "Failed to restore DJ track '{}' in guild {}: {}",
                            track_name,
                            guild_id,
                            e
                        );
                    } else {
                        tracing::info!(
                            "Successfully restored DJ track '{}' at position {:.1}s in guild {}",
                            track_name,
                            elapsed.as_secs_f32(),
                            guild_id
                        );
                    }

                    drop(manager);
                } else {
                    tracing::info!(
                        "DJ track '{}' has already finished (elapsed: {:.1}s, duration: {:.1}s), will advance to next state",
                        track_name,
                        elapsed.as_secs_f32(),
                        duration_secs
                    );
                }

                // Restore the voice channel status for the restored track if it had one
                if let Some(ref status_msg) = status_message {
                    bot_state
                        .voice_status_manager
                        .push_status(guild_id, status_msg.clone(), &http)
                        .await;
                    tracing::info!(
                        "Restored track status '{}' for DJ track in guild {}",
                        status_msg,
                        guild_id
                    );
                }
            }
            crate::persistence::DJStateMachineState::PlayingNoise { noise_profile, .. } => {
                tracing::info!(
                    "DJ was in playing noise state with profile '{}', will continue from current state",
                    noise_profile
                );
                // PlayingNoise doesn't need track restoration, it just forces a profile
            }
            _ => {}
        }
    }

    // Helper function to apply profile transition
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

    let mut current_forced_profile: Option<String> = None;

    loop {
        sleep(Duration::from_millis(DJ_TICK_INTERVAL_MS)).await;

        let track_managers = bot_state.track_managers.read().await;
        let manager_arc = track_managers
            .get(&guild_id)
            .expect("TrackManager should be initialized before DJ starts")
            .clone();
        drop(track_managers);

        let mut manager = manager_arc.lock().await;

        // Process any pending commands
        while let Ok(cmd) = command_rx.try_recv() {
            match cmd {
                DJCommand::ForceAdvance(state_type_filter) => {
                    tracing::info!("Processing force advance for DJ in guild {}", guild_id);
                    if let Err(e) = state_machine
                        .force_advance(&mut manager, &bot_state, state_type_filter)
                        .await
                    {
                        tracing::error!("DJ failed to force advance: {}", e);
                    }
                }
                DJCommand::ForceHexMessage(message) => {
                    tracing::info!("Processing force hex message for DJ in guild {}", guild_id);
                    if let Err(e) = state_machine
                        .force_hex_message(&mut manager, &bot_state, message)
                        .await
                    {
                        tracing::error!("DJ failed to force hex message: {}", e);
                    }
                }
                DJCommand::Stop => {
                    tracing::info!("Processing stop command for DJ in guild {}", guild_id);
                    state_machine.stop(&mut manager, &bot_state).await;
                }
                DJCommand::SetAnnouncementChannel(new_channel) => {
                    tracing::info!(
                        "Setting DJ announcement channel to {:?} in guild {}",
                        new_channel,
                        guild_id
                    );
                    announcement_channel = new_channel;
                    state_machine.set_announcement_channel(new_channel);
                }
                DJCommand::ReloadConfig => {
                    tracing::info!("Reloading DJ config with overrides for guild {}", guild_id);
                    // Load the base config
                    let config_path = format!("dj_configs/{}.json", config_name);
                    match DJConfig::load_from_file(&config_path) {
                        Ok(base_config) => {
                            // Apply current overrides
                            let overrides_arc = bot_state.dj_config_overrides.get_arc();
                            let overrides = overrides_arc.read().await;
                            let updated_config = base_config.with_overrides(&overrides);
                            drop(overrides);
                            
                            // Update the state machine with new config
                            state_machine.update_config(updated_config);
                            tracing::info!("DJ config reloaded successfully for guild {}", guild_id);
                        }
                        Err(e) => {
                            tracing::error!("Failed to reload DJ config for guild {}: {}", guild_id, e);
                        }
                    }
                }
            }
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

        // Persist DJ state periodically (including state machine state)
        let persist_state = crate::persistence::DJState {
            config_name: config_name.clone(),
            running: true,
            announcement_channel_id: announcement_channel.map(|id| id.get()),
            state_machine: Some(state_machine.current_state().into()),
        };
        if let Err(e) = bot_state
            .state_store
            .save_dj_state(guild_id, &persist_state)
            .await
        {
            tracing::warn!("Failed to persist DJ state for guild {}: {}", guild_id, e);
        }

        let current_state = state_machine.current_state();

        // Handle profile forcing and transitions
        if let Some(ref mut pm) = profile_machine {
            let new_forced_profile = current_state.forced_profile().map(|s| s.to_string());

            // Determine which profile to transition to, if any
            let profile_transition = if new_forced_profile != current_forced_profile {
                if let Some(ref profile_name) = new_forced_profile {
                    let fade_secs = if let DJState::PlayingNoise { duration, .. } = current_state {
                        // Fade over half the duration of the noise state
                        duration.as_secs_f32() / 2.0
                    } else {
                        1.0
                    };
                    // Force the new profile
                    pm.force_profile(profile_name.clone());
                    Some((profile_name.clone(), fade_secs, "(forced)"))
                } else if current_forced_profile.is_some() {
                    // Release the forced profile and transition to next
                    pm.release_forced_profile()
                        .map(|(profile_name, _fade_secs)| {
                            (profile_name, 1.5, "after releasing forced profile")
                        })
                } else {
                    None
                }
            } else {
                // No forced profile change, advance normally
                pm.advance()
                    .map(|(profile_name, fade_secs)| (profile_name, fade_secs, ""))
            };

            // Apply the profile transition if we have one
            if let Some((profile_name, fade_secs, reason)) = profile_transition {
                apply_profile_transition(
                    guild_id,
                    Some(&profile_name),
                    fade_secs,
                    &bot_state,
                    reason,
                )
                .await;
            }

            // Update the forced profile tracking
            if new_forced_profile != current_forced_profile {
                current_forced_profile = new_forced_profile;
            }
        }

        if let DJState::PlayingHexMessage { .. } = current_state {
            let hex_playback_states = bot_state.hex_playback_states.read().await;
            let should_advance = if let Some(state_arc) = hex_playback_states.get(&guild_id) {
                let state = state_arc.read().await;
                state.message.is_none()
            } else {
                true
            };
            drop(hex_playback_states);

            if should_advance {
                tracing::info!("DJ detected hex message completion, advancing to next state");
                if let Err(e) = state_machine
                    .force_advance(&mut manager, &bot_state, None)
                    .await
                {
                    tracing::error!("DJ failed to advance after hex message completion: {}", e);
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

    pub async fn set_announcement_channel(
        &self,
        channel_id: Option<ChannelId>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if let Some(tx) = &self.command_tx {
            tx.send(DJCommand::SetAnnouncementChannel(channel_id))
                .await?;
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
        restored_state: Option<crate::persistence::DJStateMachineState>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if self.task_handle.is_some() {
            return Err("DJ is already running".into());
        }

        // Set voice channel status if configured using the status stack
        if let Some(status) = &config.channel_status {
            bot_state
                .voice_status_manager
                .push_status(self.guild_id, status.clone(), &http)
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
        let bot_state_clone = bot_state.clone();
        let http_clone = http.clone();
        let handle = tokio::spawn(async move {
            dj_task(
                guild_id,
                config,
                bot_state_clone,
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
        let dj_state = crate::persistence::DJState {
            config_name,
            running: true,
            announcement_channel_id: announcement_channel.map(|id| id.get()),
            state_machine: Some(crate::persistence::DJStateMachineState::Idle {
                started_at: std::time::SystemTime::now(),
                duration_secs: 1.0,
            }),
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

    pub async fn force_advance(
        &self,
        state_type_filter: Option<DJStateTypeFilter>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if let Some(tx) = &self.command_tx {
            tx.send(DJCommand::ForceAdvance(state_type_filter)).await?;
            Ok(())
        } else {
            Err("DJ is not running".into())
        }
    }

    pub async fn force_hex_message(
        &self,
        message: String,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if let Some(tx) = &self.command_tx {
            tx.send(DJCommand::ForceHexMessage(message)).await?;
            Ok(())
        } else {
            Err("DJ is not running".into())
        }
    }
}

pub async fn force_advance(
    bot_state: &Data,
    guild_id: GuildId,
    state_type_filter: Option<DJStateTypeFilter>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let dj_managers = bot_state.dj_managers.read().await;
    let manager = dj_managers
        .get(&guild_id)
        .ok_or("DJ manager not found")?
        .clone();
    drop(dj_managers);

    let mgr = manager.lock().await;
    mgr.force_advance(state_type_filter).await
}

pub async fn force_hex_message(
    bot_state: &Data,
    guild_id: GuildId,
    message: String,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let dj_managers = bot_state.dj_managers.read().await;
    let manager = dj_managers
        .get(&guild_id)
        .ok_or("DJ manager not found")?
        .clone();
    drop(dj_managers);

    let mgr = manager.lock().await;
    mgr.force_hex_message(message).await
}
