use crate::audio::tracks::StartTrackArgs;
use crate::state::Data;
use poise::serenity_prelude::Http;
use std::sync::Arc;

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
    } else {
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
                    } else {
                        // Create track manager for this guild so it's available for DJ and other features
                        let _manager_arc =
                            crate::audio::tracks::get_or_create_track_manager(&bot_state, guild_id)
                                .await;
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
    }

    // Restore these features regardless of whether there are voice channels
    // since they can now operate independently of voice connections
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
        tracing::info!(
            "Restoring message playback for guild {}: '{}' at position {}",
            guild_id,
            playback_state.message,
            playback_state.current_position
        );

        let manager_arc =
            crate::audio::tracks::get_or_create_track_manager(&bot_state, guild_id).await;

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
            let playback_state_copy = playback_state_arc.clone();
            let bot_state_copy = bot_state.clone();

            let handle = tokio::spawn(async move {
                crate::audio::manager::hex_playback_task(
                    guild_id_copy,
                    manager_copy,
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
        tracing::info!(
            "Restoring {} track(s) for guild {}",
            multitrack_state.tracks.len(),
            guild_id
        );

        let manager_arc =
            crate::audio::tracks::get_or_create_track_manager(&bot_state, guild_id).await;

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
                    persist: true,
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

        // Create track manager for this guild (no longer requires voice connection)
        let _manager_arc =
            crate::audio::tracks::get_or_create_track_manager(&bot_state, guild_id).await;

        let config_path = format!("dj_configs/{}.json", dj_state.config_name);
        let dj_config = match crate::audio::dj::config::DJConfig::load_from_file(&config_path) {
            Ok(cfg) => {
                // Apply overrides if any are enabled
                let overrides_arc = bot_state.dj_config_overrides.get_arc();
                let overrides = overrides_arc.read().await;
                cfg.with_overrides(&overrides)
            }
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
