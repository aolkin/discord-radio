use crate::audio::tracks::StartTrackArgs;
use crate::state::Data;
use poise::serenity_prelude::Http;
use serenity::model::id::GuildId;
use std::sync::Arc;

// Helper: fetch the voice Call lock for a guild if connected
async fn get_call_lock(
    bot_state: &Data,
    guild_id: GuildId,
) -> Option<Arc<tokio::sync::Mutex<songbird::Call>>> {
    let voice_connections = bot_state.voice_connections.read().await;
    let lock = voice_connections.get(&guild_id).cloned();
    drop(voice_connections);
    lock
}

// Helper: get or create the TrackManager for a guild
async fn get_or_create_track_manager(
    bot_state: &Data,
    guild_id: GuildId,
    call_lock: Arc<tokio::sync::Mutex<songbird::Call>>,
) -> Arc<tokio::sync::Mutex<crate::audio::tracks::TrackManager>> {
    let mut track_managers = bot_state.track_managers.write().await;
    let manager_arc = track_managers
        .entry(guild_id)
        .or_insert_with(|| {
            Arc::new(tokio::sync::Mutex::new(
                crate::audio::tracks::TrackManager::new(call_lock, guild_id, bot_state.clone()),
            ))
        })
        .clone();

    // Link audio processor to TrackManager
    link_processor_to_manager(bot_state, guild_id, &manager_arc).await;

    manager_arc
}

// Helper: link audio processor to track manager
async fn link_processor_to_manager(
    bot_state: &Data,
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

pub async fn restore_state(
    _http: Arc<Http>,
    _bot_state: Data,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    Ok(())
}

pub async fn restore_voice_channels(
    ctx: &poise::serenity_prelude::Context,
    bot_state: Data,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let saved_channels = bot_state.state_store.load_voice_channels().await?;

    if saved_channels.is_empty() {
        tracing::info!("No saved voice channels found");
        return Ok(());
    }

    tracing::info!("Restoring {} voice channel(s)", saved_channels.len());

    let manager = songbird::get(ctx)
        .await
        .expect("Songbird Voice client placed in at initialisation.")
        .clone();

    for (guild_id, channel_id) in saved_channels {
        match manager.join(guild_id, channel_id).await {
            Ok(handle_lock) => {
                tracing::info!(
                    "Rejoined voice channel {} in guild {}",
                    channel_id,
                    guild_id
                );

                if let Err(e) = crate::audio::connection::setup_voice_connection(
                    handle_lock.clone(),
                    guild_id,
                    bot_state.clone(),
                )
                .await
                {
                    tracing::error!(
                        "Failed to setup voice connection for guild {} channel {}: {:?}",
                        guild_id,
                        channel_id,
                        e
                    );
                    let _ = bot_state.state_store.remove_voice_channel(guild_id).await;
                }
            }
            Err(e) => {
                tracing::error!(
                    "Failed to rejoin voice channel {} in guild {}: {:?}",
                    channel_id,
                    guild_id,
                    e
                );
                let _ = bot_state.state_store.remove_voice_channel(guild_id).await;
            }
        }
    }

    restore_message_playback(bot_state.clone()).await?;
    restore_multitrack_playback(bot_state.clone()).await?;
    restore_dj_managers(bot_state, ctx.http.clone()).await?;

    Ok(())
}

async fn restore_message_playback(
    bot_state: Data,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let saved_playbacks = bot_state.state_store.load_message_playbacks().await?;

    if saved_playbacks.is_empty() {
        tracing::info!("No saved message playbacks found");
        return Ok(());
    }

    tracing::info!("Restoring {} message playback(s)", saved_playbacks.len());

    for (guild_id, playback_state) in saved_playbacks {
        let call_lock = match get_call_lock(&bot_state, guild_id).await {
            Some(lock) => lock,
            None => {
                tracing::warn!(
                    "Cannot restore message playback for guild {}: not in voice channel",
                    guild_id
                );
                let _ = bot_state
                    .state_store
                    .remove_message_playback(guild_id)
                    .await;
                continue;
            }
        };

        tracing::info!(
            "Restoring message playback for guild {}: '{}' at position {}",
            guild_id,
            playback_state.message,
            playback_state.current_position
        );

        let manager_arc =
            get_or_create_track_manager(&bot_state, guild_id, call_lock.clone()).await;

        let playback_state_arc = {
            let mut states = bot_state.hex_playback_states.write().await;
            states
                .entry(guild_id)
                .or_insert_with(|| {
                    std::sync::Arc::new(tokio::sync::RwLock::new(
                        crate::state::HexPlaybackState::stopped(),
                    ))
                })
                .clone()
        };

        let tasks = bot_state.hex_playback_tasks.read().await;
        if !tasks.contains_key(&guild_id) {
            drop(tasks);

            let guild_id_copy = guild_id;
            let manager_copy = manager_arc.clone();
            let hex_audio_dir = bot_state.hex_audio_dir();
            let playback_state_copy = playback_state_arc.clone();
            let bot_state_copy = bot_state.clone();

            let handle = tokio::spawn(async move {
                crate::audio::manager::hex_playback_task(
                    guild_id_copy,
                    manager_copy,
                    hex_audio_dir,
                    playback_state_copy,
                    bot_state_copy,
                )
                .await;
            });

            bot_state
                .hex_playback_tasks
                .write()
                .await
                .insert(guild_id, handle);
        }

        {
            let mut state = playback_state_arc.write().await;
            *state = crate::state::HexPlaybackState::playing(
                playback_state.message,
                playback_state.current_position,
                1.0,
                playback_state.target_loops,
                None, // Status message will be regenerated if needed
            );
            state.current_loop = playback_state.current_loop;
        }
    }

    Ok(())
}

async fn restore_multitrack_playback(
    bot_state: Data,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let saved_multitrack = bot_state.state_store.load_multitrack_playbacks().await?;

    if saved_multitrack.is_empty() {
        tracing::info!("No saved multitrack playbacks found");
        return Ok(());
    }

    tracing::info!(
        "Restoring {} multitrack playback state(s)",
        saved_multitrack.len()
    );

    for (guild_id, multitrack_state) in saved_multitrack {
        let call_lock = match get_call_lock(&bot_state, guild_id).await {
            Some(lock) => lock,
            None => {
                tracing::warn!(
                    "Cannot restore multitrack playback for guild {}: not in voice channel",
                    guild_id
                );
                let _ = bot_state
                    .state_store
                    .remove_multitrack_playback(guild_id)
                    .await;
                continue;
            }
        };

        tracing::info!(
            "Restoring {} track(s) for guild {}",
            multitrack_state.tracks.len(),
            guild_id
        );

        let manager_arc =
            get_or_create_track_manager(&bot_state, guild_id, call_lock.clone()).await;

        let mut manager = manager_arc.lock().await;

        for track in multitrack_state.tracks {
            let start_position = if let Some(start_time) = track.start_time {
                match start_time.elapsed() {
                    Ok(elapsed) => {
                        let duration = bot_state.duration_cache.get_duration(&track.filename).await;

                        let position = if let Some(track_duration) = duration {
                            if track.loops && track_duration.as_secs() > 0 {
                                let position_in_loop =
                                    elapsed.as_secs_f64() % track_duration.as_secs_f64();
                                let seek_position =
                                    std::time::Duration::from_secs_f64(position_in_loop);
                                tracing::info!(
                                    "Track '{}' was playing for {:.2}s, seeking to {:.2}s (duration: {:.2}s)",
                                    track.name,
                                    elapsed.as_secs_f64(),
                                    position_in_loop,
                                    track_duration.as_secs_f64()
                                );
                                seek_position
                            } else {
                                tracing::info!(
                                    "Track '{}' was playing for {:.2}s before restart",
                                    track.name,
                                    elapsed.as_secs_f64()
                                );
                                elapsed
                            }
                        } else {
                            tracing::info!(
                                "Track '{}' was playing for {:.2}s before restart (duration unknown)",
                                track.name,
                                elapsed.as_secs_f64()
                            );
                            elapsed
                        };

                        Some(position)
                    }
                    Err(_) => None,
                }
            } else {
                None
            };

            let name = track.name.clone();
            let filename = track.filename;
            let volume = track.volume;
            let loops = track.loops;
            if let Err(e) = manager
                .start_track(StartTrackArgs {
                    name,
                    filename,
                    volume,
                    fade_time: 1.0,
                    loops,
                    start_position,
                })
                .await
            {
                tracing::warn!("Failed to restore track '{}': {}", track.name, e);
            }
        }
    }

    Ok(())
}

async fn restore_dj_managers(
    bot_state: Data,
    http: Arc<poise::serenity_prelude::Http>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let saved_dj_states = bot_state.state_store.load_dj_states().await?;

    if saved_dj_states.is_empty() {
        tracing::info!("No saved DJ states found");
        return Ok(());
    }

    tracing::info!("Restoring {} DJ manager(s)", saved_dj_states.len());

    for (guild_id, dj_state) in saved_dj_states {
        if !dj_state.running {
            tracing::info!("Skipping DJ for guild {} (not running)", guild_id);
            continue;
        }

        let call_lock = match get_call_lock(&bot_state, guild_id).await {
            Some(lock) => lock,
            None => {
                tracing::warn!(
                    "Cannot restore DJ for guild {}: not in voice channel",
                    guild_id
                );
                let _ = bot_state.state_store.remove_dj_state(guild_id).await;
                continue;
            }
        };

        // Get channel ID from call
        let channel_id = {
            let call = call_lock.lock().await;
            match call.current_channel() {
                Some(songbird_channel_id) => {
                    poise::serenity_prelude::ChannelId::new(songbird_channel_id.0.into())
                }
                None => {
                    tracing::warn!(
                        "Cannot restore DJ for guild {}: not in voice channel (no channel ID)",
                        guild_id
                    );
                    let _ = bot_state.state_store.remove_dj_state(guild_id).await;
                    continue;
                }
            }
        };

        // Ensure track manager exists
        let _manager_arc =
            get_or_create_track_manager(&bot_state, guild_id, call_lock.clone()).await;

        let config_path = format!("dj_configs/{}.json", dj_state.config_name);
        let dj_config = match crate::audio::dj::config::DJConfig::load_from_file(&config_path) {
            Ok(cfg) => cfg,
            Err(e) => {
                tracing::error!(
                    "Failed to load DJ config '{}' for guild {}: {}",
                    dj_state.config_name,
                    guild_id,
                    e
                );
                let _ = bot_state.state_store.remove_dj_state(guild_id).await;
                continue;
            }
        };

        tracing::info!(
            "Restoring DJ for guild {} with config '{}'",
            guild_id,
            dj_state.config_name
        );

        let mut dj_managers = bot_state.dj_managers.write().await;
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

        // Restore announcement channel if saved
        let announcement_channel = dj_state
            .announcement_channel_id
            .map(poise::serenity_prelude::ChannelId::new);

        if let Some(channel) = announcement_channel {
            tracing::info!(
                "Restoring announcement channel {} for DJ in guild {}",
                channel,
                guild_id
            );
        }

        if let Err(e) = mgr
            .start(
                dj_config,
                bot_state.clone(),
                http.clone(),
                channel_id,
                announcement_channel,
                dj_state.state_machine,
            )
            .await
        {
            tracing::error!("Failed to start DJ for guild {}: {}", guild_id, e);
            let _ = bot_state.state_store.remove_dj_state(guild_id).await;
        }
    }

    Ok(())
}
