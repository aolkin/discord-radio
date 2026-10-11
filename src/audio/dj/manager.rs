use crate::audio::dj::config::DJConfig;
use crate::audio::dj::profile_machine::ProfileStateMachine;
use crate::audio::dj::segments::{SegmentCtx, SignalProfilePlayback};
use crate::audio::dj::state_machine::{DJState, DJStateMachine, format_dj_track_name};
use crate::audio::tracks::StartTrackArgs;
use crate::state::Data;
use serenity::all::Http;
use serenity::model::id::{ChannelId, GuildId};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::time::{Duration, sleep};

const DJ_TICK_INTERVAL_MS: u64 = 100;

pub enum DJCommand {
    ForceAdvance(Option<DJStateType>),
    ForceHexMessage(String),
    ReloadConfig,
    Stop,
    SetAnnouncementChannel(Option<ChannelId>),
}

#[derive(Clone, Copy, Debug)]
pub enum DJStateType {
    Track,
    HexMessage,
    Noise,
}

struct ResumeTrackArgs {
    filename: String,
    volume: f32,
    elapsed: Duration,
    duration: Duration,
}

pub async fn dj_task(
    guild_id: GuildId,
    config: DJConfig,
    bot_state: Data,
    mut command_rx: mpsc::Receiver<DJCommand>,
    mut announcement_channel: Option<ChannelId>,
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

    let (forced_profile, status_message) = match &initial_state {
        DJState::PlayingTrack {
            filename,
            volume,
            started_at,
            duration,
            forced_profile,
            status_message,
            ..
        } => {
            resume_dj_track(
                &guild_id,
                &bot_state,
                ResumeTrackArgs {
                    filename: filename.clone(),
                    volume: *volume,
                    elapsed: started_at.elapsed(),
                    duration: *duration,
                },
            )
            .await;
            (forced_profile.clone(), status_message.clone())
        }
        DJState::PlayingHexMessage {
            forced_profile,
            status_message,
            ..
        } => (forced_profile.clone(), status_message.clone()),
        DJState::Segment(segment) => {
            let _ = segment
                .restore(&SegmentCtx {
                    guild_id: &guild_id,
                    bot_state: &bot_state,
                    http: &http,
                })
                .await
                .inspect_err(|e| tracing::warn!("Failed to restore segment: {e:?}"));
            (segment.signal_profile().name, segment.channel_status())
        }
        DJState::Stopped => (None, None),
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
        bot_state.clone(),
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

        let mut machine =
            ProfileStateMachine::new(signal_profiles, initial_profile_name.as_deref());

        // If the DJ state had a forced profile, set the machine to ForcedProfile state
        if let Some(profile_name) = forced_profile_name {
            machine.force_profile(profile_name.to_string());
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

    let mut current_forced_profile = SignalProfilePlayback::default();

    loop {
        sleep(Duration::from_millis(DJ_TICK_INTERVAL_MS)).await;

        let track_managers = bot_state.track_managers.read().await;
        let manager_arc = track_managers
            .get(&guild_id)
            .expect("TrackManager should be initialized before DJ starts")
            .clone();
        drop(track_managers);

        // Process any pending commands
        while let Ok(cmd) = command_rx.try_recv() {
            match cmd {
                DJCommand::ForceAdvance(state_type_filter) => {
                    tracing::info!("Processing force advance for DJ in guild {}", guild_id);
                    if let Err(e) = state_machine
                        .force_advance(&manager_arc, &bot_state, state_type_filter)
                        .await
                    {
                        tracing::error!("DJ failed to force advance: {}", e);
                    }
                }
                DJCommand::ForceHexMessage(message) => {
                    tracing::info!("Processing force hex message for DJ in guild {}", guild_id);
                    if let Err(e) = state_machine
                        .force_hex_message(&manager_arc, &bot_state, message)
                        .await
                    {
                        tracing::error!("DJ failed to force hex message: {}", e);
                    }
                }
                DJCommand::Stop => {
                    tracing::info!("Processing stop command for DJ in guild {}", guild_id);
                    state_machine.stop(&manager_arc, &bot_state).await;
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
                            let settings = bot_state.dj_settings.get(guild_id).await;
                            let mut config = base_config.with_overrides(&settings);
                            if let Err(e) = config
                                .apply_dj_settings(&settings, &bot_state.file_resolver)
                                .await
                            {
                                tracing::warn!(
                                    "Failed to reload DJ content for guild {guild_id}: {e:#}"
                                );
                            }
                            profile_machine = if config.signal_profiles.is_empty() {
                                None
                            } else {
                                let mut machine = ProfileStateMachine::new(
                                    config.signal_profiles.clone(),
                                    current_forced_profile.name.as_deref(),
                                );
                                if let Some(profile_name) = &current_forced_profile.name {
                                    machine.force_profile(profile_name.clone());
                                }
                                Some(machine)
                            };
                            state_machine.update_config(config);
                            tracing::info!(
                                "DJ config reloaded successfully for guild {}",
                                guild_id
                            );
                        }
                        Err(e) => {
                            tracing::error!(
                                "Failed to reload DJ config for guild {}: {}",
                                guild_id,
                                e
                            );
                        }
                    }
                }
            }
        }

        // Check for stop state
        if matches!(*state_machine.current_state().await, DJState::Stopped) {
            tracing::info!("DJ task stopped for guild {}", guild_id);
            break;
        }

        if let Err(e) = state_machine.advance(&manager_arc, &bot_state).await {
            tracing::error!("DJ state machine error in guild {}: {}", guild_id, e);
        }

        // Persist DJ state periodically (including state machine state)
        let current_state = state_machine.current_state().await;
        let persist_state = crate::persistence::DJStateSnapshot {
            config_name: &config_name,
            running: true,
            announcement_channel_id: announcement_channel.map(|id| id.get()),
            state_machine: &current_state,
        };
        if let Err(e) = bot_state
            .state_store
            .save_dj_state(guild_id, &persist_state)
            .await
        {
            tracing::warn!("Failed to persist DJ state for guild {}: {}", guild_id, e);
        }
        drop(current_state);

        // Snapshot the derived bits of the current state needed below, without holding
        // the read guard across the profile-transition awaits.
        let (new_forced_profile, is_hex_message) = {
            let current_state = state_machine.current_state().await;
            (
                current_state.forced_profile(),
                matches!(&*current_state, DJState::PlayingHexMessage { .. }),
            )
        };

        // Handle profile forcing and transitions
        if let Some(ref mut pm) = profile_machine {
            // Determine which profile to transition to, if any
            let profile_transition = if new_forced_profile.name != current_forced_profile.name {
                if let Some(ref profile) = new_forced_profile.name {
                    // Force the new profile
                    pm.force_profile(profile.clone());
                    Some((
                        profile.clone(),
                        new_forced_profile.fade_in().as_secs_f32(),
                        "(forced)",
                    ))
                } else if current_forced_profile.name.is_some() {
                    // Release the forced profile and transition to next
                    pm.release_forced_profile()
                        .map(|(profile_name, _fade_secs)| {
                            (
                                profile_name,
                                current_forced_profile.fade_out().as_secs_f32(),
                                "after releasing forced profile",
                            )
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
            if new_forced_profile.name != current_forced_profile.name {
                current_forced_profile = new_forced_profile;
            }
        }

        if is_hex_message {
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
                    .force_advance(&manager_arc, &bot_state, None)
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

async fn resume_dj_track(guild_id: &GuildId, bot_state: &Data, resume_track_args: ResumeTrackArgs) {
    let ResumeTrackArgs {
        filename,
        volume,
        elapsed,
        duration,
    } = resume_track_args;
    let track_name = format_dj_track_name(&filename);
    tracing::debug!(
        "Attempting to restore DJ track '{}' (file: {}) in guild {}",
        track_name,
        filename,
        guild_id
    );

    if elapsed < duration {
        let manager_arc = bot_state
            .track_managers
            .read()
            .await
            .get(guild_id)
            .expect("TrackManager should be initialized before DJ starts")
            .clone();
        let mut manager = manager_arc.lock().await;

        if let Err(e) = manager
            .start_track(StartTrackArgs {
                name: track_name.clone(),
                filename: filename.clone(),
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
            duration.as_secs_f32()
        );
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
        state_type_filter: Option<DJStateType>,
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
    state_type_filter: Option<DJStateType>,
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

pub async fn trigger_reload(bot_state: &Data, guild_id: GuildId) {
    let dj_managers = bot_state.dj_managers.read().await;
    let Some(manager_arc) = dj_managers.get(&guild_id) else {
        return;
    };
    let manager_arc = manager_arc.clone();
    drop(dj_managers);

    let manager = manager_arc.lock().await;
    if let Some(tx) = &manager.command_tx
        && let Err(e) = tx.send(DJCommand::ReloadConfig).await
    {
        tracing::warn!(
            "Failed to send reload command to DJ in guild {}: {}",
            guild_id,
            e
        );
    }
}
