use crate::audio::connection::connect_voice_channel;
use crate::audio::dj::manager::start_dj;
use crate::audio::tracks::StartTrackArgs;
use crate::state::Data;
use anyhow::Context;
use std::time::Duration;

pub async fn restore_voice_channels(
    ctx: &poise::serenity_prelude::Context,
    bot_state: Data,
) -> anyhow::Result<()> {
    let saved_channels = bot_state
        .state_store
        .load_voice_channels()
        .await
        .context("loading saved voice channels")?;

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
        match connect_voice_channel(&manager, guild_id, channel_id, bot_state).await {
            Ok(_) => {
                tracing::info!(
                    "Rejoined voice channel {} in guild {}",
                    channel_id,
                    guild_id
                );

                if let Err(e) = start_dj(bot_state, guild_id).await {
                    tracing::error!("Failed to start DJ for guild {}: {e:#}", guild_id);
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

    // Restore these features regardless of whether there are voice channels
    // since they can now operate independently of voice connections
    restore_multitrack_playback(bot_state).await?;

    Ok(())
}

async fn restore_multitrack_playback(bot_state: Data) -> anyhow::Result<()> {
    let saved_multitrack = bot_state
        .state_store
        .load_multitrack_playbacks()
        .await
        .context("loading saved multitrack playbacks")?;

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

        let manager = bot_state.track_manager(guild_id).await;

        for track in multitrack_state.tracks {
            let start_position = if let Some(start_time) = track.start_time {
                match start_time.elapsed() {
                    Ok(elapsed) => {
                        let duration = bot_state.duration_cache.get_duration(&track.filename).await;

                        let position = if let Some(track_duration) = duration {
                            if track.loops && track_duration.as_secs() > 0 {
                                let position_in_loop =
                                    elapsed.as_secs_f64() % track_duration.as_secs_f64();
                                let seek_position = Duration::from_secs_f64(position_in_loop);
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
