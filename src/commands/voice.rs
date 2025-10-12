use crate::commands::utils;
use crate::commands::utils::{Context, Error};
use serenity::all::{ChannelId, ChannelType, GuildId};
use std::collections::HashMap;
use std::sync::Arc;

/// Join a voice channel to broadcast audio
#[poise::command(
    slash_command,
    guild_only,
    default_member_permissions = "ADMINISTRATOR"
)]
pub async fn join_voice_channel(
    ctx: Context<'_>,
    #[description = "Voice channel to join and broadcast audio"] channel: ChannelId,
) -> Result<(), Error> {
    let guild_id = ctx
        .guild_id()
        .ok_or("This command can only be used in a server")?;
    let user_id = ctx.author().id;

    tracing::info!(
        "User {} executed join_voice_channel for channel {} in guild {}",
        user_id,
        channel,
        guild_id
    );

    ctx.defer_ephemeral().await?;

    let channel_info = utils::get_channel_details(ctx, channel).await?;

    if channel_info
        .guild()
        .is_none_or(|guild_channel| guild_channel.kind != ChannelType::Voice)
    {
        tracing::warn!("Channel {} is not a voice channel", channel);
        ctx.say("The specified channel is not a voice channel!")
            .await?;
        return Ok(());
    }

    if let Err(e) = join_voice_channel_helper(ctx, guild_id, channel).await {
        ctx.say(e).await?;
        return Ok(());
    }

    let _ = ctx
        .data()
        .state_store
        .save_voice_channel(guild_id, channel)
        .await;

    ctx.say(format!("Joined voice channel <#{}>", channel))
        .await?;

    Ok(())
}

/// Leave the current voice channel
#[poise::command(
    slash_command,
    guild_only,
    default_member_permissions = "ADMINISTRATOR"
)]
pub async fn leave_voice_channel(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx
        .guild_id()
        .ok_or("This command can only be used in a server")?;
    let user_id = ctx.author().id;

    tracing::info!(
        "User {} executed leave_voice_channel in guild {}",
        user_id,
        guild_id
    );

    ctx.defer_ephemeral().await?;

    let manager = songbird::get(ctx.serenity_context())
        .await
        .expect("Songbird Voice client placed in at initialisation.")
        .clone();

    if manager.get(guild_id).is_some() {
        if let Err(e) = manager.remove(guild_id).await {
            ctx.say(format!("Failed to leave voice channel: {}", e))
                .await?;
        } else {
            let _ = ctx.data().state_store.remove_voice_channel(guild_id).await;
            ctx.say("Left voice channel").await?;
        }
    } else {
        ctx.say("Not in a voice channel").await?;
    }

    Ok(())
}

/// Convert a text message to hex and play it as audio in voice
#[poise::command(
    slash_command,
    guild_only,
    default_member_permissions = "ADMINISTRATOR"
)]
pub async fn play_message(
    ctx: Context<'_>,
    #[description = "Message to convert to hex and play"] message: String,
    #[description = "Voice channel to join (optional)"] channel: Option<ChannelId>,
    #[description = "Volume 0.0-1.0 (default 1.0)"] volume: Option<f32>,
    #[description = "Number of times to loop the message (default infinite)"] loops: Option<u32>,
) -> Result<(), Error> {
    let guild_id = ctx
        .guild_id()
        .ok_or("This command can only be used in a server")?;
    let user_id = ctx.author().id;

    tracing::info!(
        "User {} executed play_message with message '{}' in guild {}",
        user_id,
        message,
        guild_id
    );

    let reply = ctx
        .send(
            poise::CreateReply::default()
                .content("Playing message...")
                .ephemeral(true),
        )
        .await?;

    if let Some(channel_id) = channel {
        let channel_info = utils::get_channel_details(ctx, channel_id).await?;

        if channel_info
            .guild()
            .is_none_or(|guild_channel| guild_channel.kind != ChannelType::Voice)
        {
            tracing::warn!("Channel {} is not a voice channel", channel_id);
            reply
                .edit(
                    ctx,
                    poise::CreateReply::default()
                        .content("The specified channel is not a voice channel!")
                        .ephemeral(true),
                )
                .await?;
            return Ok(());
        }

        if let Err(e) = join_voice_channel_helper(ctx, guild_id, channel_id).await {
            reply
                .edit(
                    ctx,
                    poise::CreateReply::default().content(e).ephemeral(true),
                )
                .await?;
            return Ok(());
        }
    }

    let call_lock = match get_voice_connection(ctx, guild_id).await {
        Some(lock) => lock,
        None => {
            reply
                .edit(
                    ctx,
                    poise::CreateReply::default()
                        .content(
                            "Not in a voice channel! Use the channel parameter to join one first.",
                        )
                        .ephemeral(true),
                )
                .await?;
            return Ok(());
        }
    };

    let volume = volume.unwrap_or(1.0);

    let manager_arc = get_or_create_track_manager(ctx, guild_id, call_lock.clone()).await;
    let playback_state = get_or_create_hex_playback_state(ctx, guild_id).await;

    ensure_hex_playback_task(ctx, guild_id, manager_arc.clone(), playback_state.clone()).await;

    {
        let mut state = playback_state.write().await;
        *state = crate::state::HexPlaybackState::playing(
            message.clone(),
            0,
            volume,
            loops.map(|l| l as usize),
        );
    }

    let initial_state = crate::persistence::MessagePlaybackState {
        message: message.clone(),
        current_position: 0,
        current_loop: 0,
        target_loops: loops.map(|l| l as usize),
    };
    if let Err(e) = ctx
        .data()
        .state_store
        .save_message_playback(guild_id, &initial_state)
        .await
    {
        tracing::warn!("Failed to save initial message playback state: {}", e);
    }

    let voice_channel_id = {
        let call = call_lock.lock().await;
        call.current_channel()
            .map(|id| serenity::model::id::ChannelId::new(id.0.get()))
    };

    if let Some(channel_id) = voice_channel_id {
        let obfuscated = obfuscate_message(&message);
        if let Err(e) = channel_id
            .edit(
                ctx.http(),
                serenity::builder::EditChannel::new().status(obfuscated),
            )
            .await
        {
            tracing::warn!("Failed to set voice channel status: {}", e);
        }
    }

    reply
        .edit(
            ctx,
            poise::CreateReply::default()
                .content(format!(
                    "Now playing message in loop: \"{}\"\nUse /stop_message to stop playback.",
                    message
                ))
                .ephemeral(true),
        )
        .await?;

    Ok(())
}

/// Stop the currently playing message
#[poise::command(
    slash_command,
    guild_only,
    default_member_permissions = "ADMINISTRATOR"
)]
pub async fn stop_message(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx
        .guild_id()
        .ok_or("This command can only be used in a server")?;
    let user_id = ctx.author().id;

    tracing::info!(
        "User {} executed stop_message in guild {}",
        user_id,
        guild_id
    );

    ctx.defer_ephemeral().await?;

    let is_playing = {
        let states = ctx.data().hex_playback_states.read().await;
        if let Some(state_arc) = states.get(&guild_id) {
            let state = state_arc.read().await;
            state.message.is_some()
        } else {
            false
        }
    };

    if is_playing {
        stop_hex_playback(ctx, guild_id).await;
        ctx.say("Message playback stopped").await?;
    } else {
        ctx.say("No message is currently playing").await?;
    }

    Ok(())
}

async fn autocomplete_audio_file(ctx: Context<'_>, partial: &'_ str) -> Vec<String> {
    let content_dir = &ctx.data().content_path;
    let mut results = Vec::new();

    // Split partial into directory and filename parts
    let (dir_part, file_part) = if let Some(pos) = partial.rfind('/') {
        (&partial[..=pos], &partial[pos + 1..])
    } else {
        ("", partial)
    };

    // Build the full path to search
    let search_path = if dir_part.is_empty() {
        std::path::PathBuf::from(content_dir)
    } else {
        std::path::PathBuf::from(format!("{}/{}", content_dir, dir_part))
    };

    if let Ok(entries) = std::fs::read_dir(&search_path) {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(name) = path.file_name().and_then(|n| n.to_str())
                && name.starts_with(file_part)
            {
                let full_name = if dir_part.is_empty() {
                    name.to_string()
                } else {
                    format!("{}{}", dir_part, name)
                };

                if path.is_dir() {
                    results.push(format!("{}/", full_name));
                } else if let Some(ext) = path.extension() {
                    let ext_str = ext.to_string_lossy().to_lowercase();
                    if matches!(ext_str.as_str(), "mp3" | "ogg" | "wav" | "flac" | "m4a") {
                        results.push(full_name);
                    }
                }
            }
        }
    }

    results.sort();
    results.truncate(25);
    results
}

/// Start or stop an audio track with fade transition
#[poise::command(
    slash_command,
    guild_only,
    default_member_permissions = "ADMINISTRATOR"
)]
pub async fn change_track_state(
    ctx: Context<'_>,
    #[description = "Track name identifier"] name: String,
    #[description = "Track state: start or stop (omit to update existing)"] state: Option<String>,
    #[description = "Audio filename (required for start)"]
    #[autocomplete = "autocomplete_audio_file"]
    filename: Option<String>,
    #[description = "Volume 0.0-1.0"] volume: Option<f32>,
    #[description = "Loop track"] loops: Option<bool>,
    #[description = "Fade time in seconds (default 1.0)"] fade_time: Option<f32>,
) -> Result<(), Error> {
    let guild_id = ctx
        .guild_id()
        .ok_or("This command can only be used in a server")?;

    ctx.defer_ephemeral().await?;

    let call_lock = match get_voice_connection(ctx, guild_id).await {
        Some(lock) => lock,
        None => {
            ctx.say("Not in a voice channel!").await?;
            return Ok(());
        }
    };

    let manager_arc = get_or_create_track_manager(ctx, guild_id, call_lock).await;
    let mut manager = manager_arc.lock().await;
    let fade_time = fade_time.unwrap_or(1.0);

    let state_str = state.as_deref().map(|s| s.to_lowercase());
    match state_str.as_deref() {
        Some("start") => {
            let Some(filename) = filename else {
                ctx.say("filename is required for start").await?;
                return Ok(());
            };

            // Prepend content directory to filename
            let full_path = format!("{}/{}", ctx.data().content_path, filename);

            let volume = volume.unwrap_or(1.0);
            let loops = loops.unwrap_or(true);

            // If track exists, fade it out in the background
            if manager.has_track(&name) {
                let manager_clone = manager_arc.clone();
                let name_clone = name.clone();
                tokio::spawn(async move {
                    let mut mgr = manager_clone.lock().await;
                    if let Err(e) = mgr.stop_track(&name_clone, fade_time, false).await {
                        tracing::warn!("Failed to stop existing track '{}': {}", name_clone, e);
                    }
                });
            }

            if let Err(e) = manager
                .start_track(crate::audio::tracks::StartTrackArgs {
                    name: name.clone(),
                    filename: full_path,
                    volume,
                    fade_time,
                    loops,
                    start_position: None,
                })
                .await
            {
                ctx.say(format!("Failed to start track: {}", e)).await?;
            } else {
                ctx.say(format!(
                    "Started track '{}' with volume {}, {} second fade, looping: {}",
                    name, volume, fade_time, loops
                ))
                .await?;
            }
        }
        Some("stop") => {
            if let Err(e) = manager.stop_track(&name, fade_time, true).await {
                ctx.say(format!("Failed to stop track: {}", e)).await?;
            } else {
                ctx.say(format!(
                    "Stopped track '{}' with {} second fade",
                    name, fade_time
                ))
                .await?;
            }
        }
        None => {
            if !manager.has_track(&name) {
                ctx.say(format!(
                    "Track '{}' not found. Use state=start to create a new track.",
                    name
                ))
                .await?;
                return Ok(());
            }

            let mut updated = Vec::new();

            if let Some(new_volume) = volume {
                if let Err(e) = manager
                    .update_track_volume(&name, new_volume, fade_time)
                    .await
                {
                    ctx.say(format!("Failed to update volume: {}", e)).await?;
                    return Ok(());
                }
                updated.push(format!("volume to {}", new_volume));
            }

            if let Some(new_loops) = loops {
                if let Err(e) = manager.update_track_loops(&name, new_loops).await {
                    ctx.say(format!("Failed to update loops: {}", e)).await?;
                    return Ok(());
                }
                updated.push(format!("loops to {}", new_loops));
            }

            if updated.is_empty() {
                ctx.say("No updates specified. Provide volume and/or loops to update.")
                    .await?;
            } else {
                ctx.say(format!("Updated track '{}': {}", name, updated.join(", ")))
                    .await?;
            }
        }
        _ => {
            ctx.say("Invalid state. Use 'start' or 'stop'").await?;
        }
    }

    Ok(())
}

/// Display all currently playing audio tracks
#[poise::command(
    slash_command,
    guild_only,
    default_member_permissions = "ADMINISTRATOR"
)]
pub async fn get_current_tracks(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx
        .guild_id()
        .ok_or("This command can only be used in a server")?;

    let track_managers = ctx.data().track_managers.read().await;

    let tracks = match track_managers.get(&guild_id) {
        Some(manager_arc) => {
            let manager = manager_arc.lock().await;
            manager.get_all_tracks()
        }
        None => Vec::new(),
    };

    ctx.defer_ephemeral().await?;

    if tracks.is_empty() {
        ctx.say("No tracks currently playing").await?;
        return Ok(());
    }

    let embed = serenity::all::CreateEmbed::default()
        .title("Currently Playing Tracks")
        .description(format!("{} track(s) active", tracks.len()))
        .fields(tracks.iter().map(|track| {
            (
                &track.name,
                format!(
                    "File: {}\nVolume: {:.2}\nLoops: {}",
                    track.filename, track.volume, track.loops
                ),
                false,
            )
        }))
        .color(0x00ff00);

    ctx.send(poise::CreateReply::default().embed(embed).ephemeral(true))
        .await?;

    Ok(())
}

async fn stop_hex_playback(ctx: Context<'_>, guild_id: GuildId) {
    let hex_playback_states = ctx.data().hex_playback_states.read().await;
    if let Some(state_arc) = hex_playback_states.get(&guild_id) {
        let mut state = state_arc.write().await;
        *state = crate::state::HexPlaybackState::stopped();
        drop(state);
        drop(hex_playback_states);

        let track_managers = ctx.data().track_managers.read().await;
        if let Some(manager_arc) = track_managers.get(&guild_id) {
            let mut manager = manager_arc.lock().await;
            let _ = manager
                .stop_track(crate::audio::manager::HEX_PLAYBACK_TRACK_NAME, 0.0, true)
                .await;
        }

        if let Err(e) = ctx
            .data()
            .state_store
            .remove_message_playback(guild_id)
            .await
        {
            tracing::warn!("Failed to remove message playback state: {}", e);
        }

        clear_voice_channel_status(ctx.http(), &ctx.data().voice_connections, guild_id).await;
    }
}

async fn join_voice_channel_helper(
    ctx: Context<'_>,
    guild_id: GuildId,
    channel_id: ChannelId,
) -> Result<(), String> {
    let manager = songbird::get(ctx.serenity_context())
        .await
        .expect("Songbird Voice client placed in at initialisation.")
        .clone();

    match manager.join(guild_id, channel_id).await {
        Ok(handle_lock) => {
            tracing::info!("Joined voice channel");

            if let Err(e) = crate::audio::connection::setup_voice_connection(
                handle_lock,
                guild_id,
                ctx.data().clone(),
            )
            .await
            {
                return Err(format!("Failed to setup voice connection: {}", e));
            }

            Ok(())
        }
        Err(e) => Err(format!("Failed to join the voice channel: {}", e)),
    }
}

async fn get_voice_connection(
    ctx: Context<'_>,
    guild_id: GuildId,
) -> Option<Arc<tokio::sync::Mutex<songbird::Call>>> {
    let voice_connections = ctx.data().voice_connections.read().await;
    voice_connections.get(&guild_id).map(Arc::clone)
}

async fn get_or_create_track_manager(
    ctx: Context<'_>,
    guild_id: GuildId,
    call_lock: Arc<tokio::sync::Mutex<songbird::Call>>,
) -> Arc<tokio::sync::Mutex<crate::audio::tracks::TrackManager>> {
    let mut track_managers = ctx.data().track_managers.write().await;
    let manager_arc = track_managers
        .entry(guild_id)
        .or_insert_with(|| {
            Arc::new(tokio::sync::Mutex::new(
                crate::audio::tracks::TrackManager::new(
                    call_lock.clone(),
                    guild_id,
                    ctx.data().clone(),
                ),
            ))
        })
        .clone();

    // Link audio processor if available
    link_processor_to_manager(ctx.data(), guild_id, &manager_arc).await;

    manager_arc
}

async fn link_processor_to_manager(
    bot_state: &crate::state::Data,
    guild_id: GuildId,
    manager_arc: &Arc<tokio::sync::Mutex<crate::audio::tracks::TrackManager>>,
) {
    if let Some(processor_arc) = bot_state
        .audio_processors
        .read()
        .await
        .get(&guild_id)
        .cloned()
    {
        let mut manager = manager_arc.lock().await;
        manager.set_audio_processor(processor_arc);
    }
}

async fn get_or_create_hex_playback_state(
    ctx: Context<'_>,
    guild_id: GuildId,
) -> Arc<tokio::sync::RwLock<crate::state::HexPlaybackState>> {
    let mut states = ctx.data().hex_playback_states.write().await;
    states
        .entry(guild_id)
        .or_insert_with(|| {
            Arc::new(tokio::sync::RwLock::new(
                crate::state::HexPlaybackState::stopped(),
            ))
        })
        .clone()
}

async fn ensure_hex_playback_task(
    ctx: Context<'_>,
    guild_id: GuildId,
    manager_arc: Arc<tokio::sync::Mutex<crate::audio::tracks::TrackManager>>,
    playback_state: Arc<tokio::sync::RwLock<crate::state::HexPlaybackState>>,
) {
    let tasks = ctx.data().hex_playback_tasks.read().await;
    if tasks.contains_key(&guild_id) {
        return;
    }
    drop(tasks);

    let guild_id_copy = guild_id;
    let manager_copy = manager_arc.clone();
    let hex_audio_dir = ctx.data().hex_audio_dir();
    let playback_state_copy = playback_state.clone();
    let bot_state = ctx.data().clone();

    let handle = tokio::spawn(async move {
        crate::audio::manager::hex_playback_task(
            guild_id_copy,
            manager_copy,
            hex_audio_dir,
            playback_state_copy,
            bot_state,
        )
        .await;
    });

    ctx.data()
        .hex_playback_tasks
        .write()
        .await
        .insert(guild_id, handle);
}

async fn autocomplete_profile(ctx: Context<'_>, partial: &'_ str) -> Vec<String> {
    let mut profiles = Vec::new();

    // Add special "bypass" profile
    if "bypass".starts_with(partial) {
        profiles.push("bypass".to_string());
    }

    profiles.extend(
        ctx.data()
            .profile_manager
            .list_profiles()
            .into_iter()
            .filter(|name| name.starts_with(partial)),
    );

    profiles.sort();
    profiles
}

/// Change the audio signal processing profile
#[poise::command(
    slash_command,
    guild_only,
    default_member_permissions = "ADMINISTRATOR"
)]
pub async fn signal_profile(
    ctx: Context<'_>,
    #[description = "Profile name"]
    #[autocomplete = "autocomplete_profile"]
    profile: String,
    #[description = "Fade duration in seconds (default: 2.0)"] fade_duration: Option<f32>,
) -> Result<(), Error> {
    let guild_id = ctx
        .guild_id()
        .ok_or("This command can only be used in a server")?;
    let user_id = ctx.author().id;

    tracing::info!(
        "User {} executed signal_profile with profile '{}' and fade_duration {:?} in guild {}",
        user_id,
        profile,
        fade_duration,
        guild_id
    );

    ctx.defer_ephemeral().await?;

    let fade_duration_ms = fade_duration.unwrap_or(2.0) * 1000.0;

    if fade_duration_ms < 0.0 {
        ctx.say("Fade duration must be non-negative").await?;
        return Ok(());
    }

    let processors = ctx.data().audio_processors.read().await;
    let processor_arc = processors.get(&guild_id).cloned();
    drop(processors);

    if let Some(processor_arc) = processor_arc {
        // Handle special "bypass" profile
        if profile == "bypass" {
            let mut processor = processor_arc.write().await;
            processor.set_bypass(true);
            drop(processor);

            // Persist bypass state
            let profile_state = crate::persistence::ProfileState {
                profile_name: "bypass".to_string(),
                bypass: true,
            };
            if let Err(e) = ctx
                .data()
                .state_store
                .save_profile_state(guild_id, &profile_state)
                .await
            {
                tracing::warn!("Failed to save profile state: {}", e);
            }

            ctx.say("DSP bypass enabled - audio will pass through unprocessed")
                .await?;
            return Ok(());
        }

        // Load regular profile from ProfileManager
        if let Some(new_profile) = ctx.data().profile_manager.get_profile(&profile) {
            let mut processor = processor_arc.write().await;

            if fade_duration_ms > 0.0 {
                processor.start_profile_transition(new_profile.clone(), fade_duration_ms);
            } else {
                processor.set_profile_immediate(new_profile.clone());
            }

            drop(processor);

            // Persist profile state
            let profile_state = crate::persistence::ProfileState {
                profile_name: profile.clone(),
                bypass: false,
            };
            if let Err(e) = ctx
                .data()
                .state_store
                .save_profile_state(guild_id, &profile_state)
                .await
            {
                tracing::warn!("Failed to save profile state: {}", e);
            }

            ctx.say(format!(
                "Switched to signal profile '{}' with {}s fade",
                profile,
                fade_duration_ms / 1000.0
            ))
            .await?;
        } else {
            ctx.say(format!("Profile '{}' could not be loaded", profile))
                .await?;
        }
    } else {
        ctx.say(
            "Audio processor not initialized for this guild.\n\
                Note: Full DSP integration requires refactoring the track system.",
        )
        .await?;
    }

    Ok(())
}

pub fn obfuscate_message(message: &str) -> String {
    use rand::Rng;
    let mut rng = rand::rng();

    message
        .chars()
        .map(|c| {
            if c.is_alphanumeric() {
                if rng.random_bool(0.5) { '?' } else { '¿' }
            } else {
                c
            }
        })
        .collect()
}

async fn clear_voice_channel_status(
    http: &serenity::http::Http,
    voice_connections: &tokio::sync::RwLock<
        HashMap<GuildId, Arc<tokio::sync::Mutex<songbird::Call>>>,
    >,
    guild_id: GuildId,
) {
    let voice_connections_guard = voice_connections.read().await;
    if let Some(call_lock) = voice_connections_guard.get(&guild_id) {
        let call = call_lock.lock().await;
        if let Some(songbird_channel_id) = call.current_channel() {
            let channel_id = serenity::model::id::ChannelId::new(songbird_channel_id.0.get());
            if let Err(e) = channel_id
                .edit(http, serenity::builder::EditChannel::new().status(""))
                .await
            {
                tracing::warn!("Failed to clear voice channel status: {}", e);
            }
        }
    }
}

/// Manage the radio DJ for automated playback
#[poise::command(
    slash_command,
    guild_only,
    default_member_permissions = "ADMINISTRATOR"
)]
pub async fn manage_dj(
    ctx: Context<'_>,
    #[description = "DJ configuration name"] config: String,
    #[description = "Action: start or stop"] action: String,
    #[description = "Text channel for announcements (optional)"] announcement_channel: Option<
        ChannelId,
    >,
) -> Result<(), Error> {
    let guild_id = ctx
        .guild_id()
        .ok_or("This command can only be used in a server")?;
    let user_id = ctx.author().id;

    tracing::info!(
        "User {} executed manage_dj with config '{}' and action '{}' in guild {}",
        user_id,
        config,
        action,
        guild_id
    );

    ctx.defer_ephemeral().await?;

    let action_lower = action.to_lowercase();

    match action_lower.as_str() {
        "start" => {
            let config_path = format!("dj_configs/{}.json", config);
            let dj_config = match crate::audio::dj::config::DJConfig::load_from_file(&config_path) {
                Ok(cfg) => cfg,
                Err(e) => {
                    ctx.say(format!("Failed to load DJ config '{}': {}", config, e))
                        .await?;
                    return Ok(());
                }
            };

            // Ensure we have a voice connection and track manager before starting DJ
            let call_lock = match get_voice_connection(ctx, guild_id).await {
                Some(lock) => lock,
                None => {
                    ctx.say("Bot is not in a voice channel! Join a voice channel first.")
                        .await?;
                    return Ok(());
                }
            };

            // Get the channel ID from the call
            let channel_id = {
                let call = call_lock.lock().await;
                match call.current_channel() {
                    Some(songbird_channel_id) => {
                        serenity::all::ChannelId::new(songbird_channel_id.0.into())
                    }
                    None => {
                        ctx.say("Bot is not in a voice channel! Join a voice channel first.")
                            .await?;
                        return Ok(());
                    }
                }
            };

            // Create track manager if it doesn't exist
            let _track_manager = get_or_create_track_manager(ctx, guild_id, call_lock).await;

            let mut dj_managers = ctx.data().dj_managers.write().await;
            let manager = dj_managers
                .entry(guild_id)
                .or_insert_with(|| {
                    Arc::new(tokio::sync::Mutex::new(
                        crate::audio::dj::manager::DJManager::new(guild_id),
                    ))
                })
                .clone();
            drop(dj_managers);

            let mut mgr = manager.lock().await;
            if mgr.is_running() {
                ctx.say(
                    "DJ is already running! Stop it first before starting a new configuration.",
                )
                .await?;
                return Ok(());
            }

            if let Err(e) = mgr
                .start(
                    dj_config,
                    ctx.data().clone(),
                    ctx.serenity_context().http.clone(),
                    channel_id,
                    announcement_channel,
                    None,
                )
                .await
            {
                ctx.say(format!("Failed to start DJ: {}", e)).await?;
                return Ok(());
            }

            let channel_msg = if let Some(ch_id) = announcement_channel {
                format!(" (announcements in <#{}>)", ch_id)
            } else {
                String::new()
            };

            ctx.say(format!(
                "DJ started with configuration '{}'{}",
                config, channel_msg
            ))
            .await?;
        }
        "stop" => {
            let dj_managers = ctx.data().dj_managers.read().await;
            let manager = match dj_managers.get(&guild_id) {
                Some(mgr) => mgr.clone(),
                None => {
                    ctx.say("DJ is not running").await?;
                    return Ok(());
                }
            };
            drop(dj_managers);

            let mut mgr = manager.lock().await;
            if !mgr.is_running() {
                ctx.say("DJ is not running").await?;
                return Ok(());
            }

            mgr.stop(ctx.data(), ctx.serenity_context().http.clone())
                .await;
            ctx.say("DJ stopped").await?;
        }
        "configure" => {
            let dj_managers = ctx.data().dj_managers.read().await;
            let manager = match dj_managers.get(&guild_id) {
                Some(mgr) => mgr.clone(),
                None => {
                    ctx.say("DJ is not running").await?;
                    return Ok(());
                }
            };
            drop(dj_managers);

            let mgr = manager.lock().await;
            if !mgr.is_running() {
                ctx.say("DJ is not running").await?;
                return Ok(());
            }

            if let Err(e) = mgr.set_announcement_channel(announcement_channel).await {
                ctx.say(format!("Failed to configure DJ: {}", e)).await?;
                return Ok(());
            }

            let msg = if let Some(ch_id) = announcement_channel {
                format!("DJ configured: announcements will be sent to <#{}>", ch_id)
            } else {
                "DJ configured: announcements disabled".to_string()
            };

            ctx.say(msg).await?;
        }
        _ => {
            ctx.say("Invalid action. Use 'start', 'stop', or 'configure'")
                .await?;
        }
    }

    Ok(())
}

/// Get the current state of the DJ
#[poise::command(
    slash_command,
    guild_only,
    default_member_permissions = "ADMINISTRATOR"
)]
pub async fn get_dj_state(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx
        .guild_id()
        .ok_or("This command can only be used in a server")?;
    let user_id = ctx.author().id;

    tracing::info!(
        "User {} executed get_dj_state in guild {}",
        user_id,
        guild_id
    );

    ctx.defer_ephemeral().await?;

    let dj_managers = ctx.data().dj_managers.read().await;
    if !dj_managers.contains_key(&guild_id) {
        ctx.say("DJ is not running").await?;
        return Ok(());
    }
    drop(dj_managers);

    let dj_states = ctx.data().dj_states.read().await;
    let state_arc = match dj_states.get(&guild_id) {
        Some(arc) => arc.clone(),
        None => {
            ctx.say("DJ state not available").await?;
            return Ok(());
        }
    };
    drop(dj_states);

    let state = state_arc.read().await;
    let state_description = match &*state {
        crate::audio::dj::state_machine::DJState::PlayingTrack {
            filename,
            started_at,
            duration,
            ..
        } => {
            let elapsed = started_at.elapsed().as_secs();
            let total = duration.as_secs();
            format!("Playing track: **{}** ({}/{}s)", filename, elapsed, total)
        }
        crate::audio::dj::state_machine::DJState::PlayingHexMessage {
            started_at,
            target_loops,
            message,
        } => {
            let elapsed = started_at.elapsed().as_secs();

            // Try to get current loop from hex playback state
            if let Some((current_loop, current_position, current_message)) = {
                let hex_states = ctx.data().hex_playback_states.read().await;
                if let Some(state_arc) = hex_states.get(&guild_id) {
                    let state = state_arc.read().await;
                    Some((
                        state.current_loop,
                        state.current_position,
                        state.message.clone(),
                    ))
                } else {
                    None
                }
            } {
                let msg_len = current_message.as_ref().map(|m| m.len()).unwrap_or(0);
                let msg_display = current_message.as_deref().unwrap_or("-");
                format!(
                    "Playing hex message: **{}** (position {}/{}, loop {}/{}, {}s elapsed)",
                    msg_display,
                    current_position,
                    msg_len * 2,
                    current_loop + 1,
                    target_loops,
                    elapsed
                )
            } else {
                format!(
                    "Playing hex message: **{}** ({} loops, {}s elapsed)",
                    message, target_loops, elapsed
                )
            }
        }
        crate::audio::dj::state_machine::DJState::PlayingNoise {
            noise_type,
            started_at,
            duration,
        } => {
            let elapsed = started_at.elapsed().as_secs();
            let total = duration.as_secs();
            format!("Playing noise: **{}** ({}/{}s)", noise_type, elapsed, total)
        }
        crate::audio::dj::state_machine::DJState::TransitioningProfile {
            started_at,
            duration,
        } => {
            let elapsed = started_at.elapsed().as_secs();
            let total = duration.as_secs();
            format!("Transitioning profile ({}/{}s)", elapsed, total)
        }
        crate::audio::dj::state_machine::DJState::Idle {
            started_at,
            duration,
        } => {
            let elapsed = started_at.elapsed().as_secs();
            let total = duration.as_secs();
            format!("Idle ({}/{}s)", elapsed, total)
        }
        crate::audio::dj::state_machine::DJState::Stopped => "Stopped".to_string(),
    };

    ctx.say(format!("DJ State: {}", state_description)).await?;

    Ok(())
}

/// Force the DJ to advance to the next state (for testing)
#[poise::command(
    slash_command,
    guild_only,
    default_member_permissions = "ADMINISTRATOR"
)]
pub async fn advance_dj_state(
    ctx: Context<'_>,
    #[description = "State type to advance to: track, hex, or noise (optional)"] state_type: Option<
        String,
    >,
) -> Result<(), Error> {
    let guild_id = ctx
        .guild_id()
        .ok_or("This command can only be used in a server")?;
    let user_id = ctx.author().id;

    tracing::info!(
        "User {} executed advance_dj_state in guild {}",
        user_id,
        guild_id
    );

    ctx.defer_ephemeral().await?;

    let dj_managers = ctx.data().dj_managers.read().await;
    let manager = match dj_managers.get(&guild_id) {
        Some(mgr) => mgr.clone(),
        None => {
            ctx.say("DJ is not running").await?;
            return Ok(());
        }
    };
    drop(dj_managers);

    let mgr = manager.lock().await;
    if !mgr.is_running() {
        ctx.say("DJ is not running").await?;
        return Ok(());
    }
    drop(mgr);

    // Parse state type filter
    let state_type_filter = if let Some(type_str) = state_type {
        match type_str.to_lowercase().as_str() {
            "track" => Some(crate::audio::dj::manager::DJStateTypeFilter::Track),
            "hex" | "hex_message" | "hexmessage" => {
                Some(crate::audio::dj::manager::DJStateTypeFilter::HexMessage)
            }
            "noise" => Some(crate::audio::dj::manager::DJStateTypeFilter::Noise),
            _ => {
                ctx.say(format!(
                    "Invalid state type '{}'. Valid types: track, hex, noise",
                    type_str
                ))
                .await?;
                return Ok(());
            }
        }
    } else {
        None
    };

    if let Err(e) =
        crate::audio::dj::manager::force_advance(ctx.data(), guild_id, state_type_filter).await
    {
        ctx.say(format!("Failed to force DJ state advance: {}", e))
            .await?;
        return Ok(());
    }

    let message = if let Some(filter) = state_type_filter {
        format!("DJ advancing to {:?} state", filter)
    } else {
        "DJ state advance triggered".to_string()
    };

    ctx.say(message).await?;

    Ok(())
}
