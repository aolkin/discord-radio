use crate::audio::tracks::StartTrackArgs;
use crate::persistence::MultiTrackPlaybackState;
use crate::state::Data;
use anyhow::Context;
use serenity::model::id::GuildId;
use std::time::Duration;
use tracing::Instrument;

pub async fn restore_voice_channels(bot_state: Data) -> anyhow::Result<()> {
    let saved_channels = bot_state
        .state_store
        .load_voice_channels()
        .await
        .context("loading saved voice channels")?;

    if saved_channels.is_empty() {
        tracing::info!("No saved voice channels found");
    } else {
        tracing::info!("Restoring {} voice channel(s)", saved_channels.len());
    }

    for (guild_id, channel_id) in saved_channels {
        let span = tracing::info_span!("restore_voice", guild_id = %guild_id);
        async {
            match bot_state.join_voice_channel(guild_id, channel_id).await {
                Ok(()) => tracing::info!("Rejoined voice channel {}", channel_id),
                Err(e) => {
                    tracing::error!("Failed to rejoin voice channel {}: {:?}", channel_id, e);
                    let _ = bot_state.state_store.remove_voice_channel(guild_id).await;
                }
            }
        }
        .instrument(span)
        .await;
    }

    Ok(())
}

pub async fn restore_multitrack_playback(bot_state: Data) -> anyhow::Result<()> {
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
        restore_guild_multitrack(bot_state, guild_id, multitrack_state).await;
    }

    Ok(())
}

#[tracing::instrument(skip_all, fields(guild_id = %guild_id))]
async fn restore_guild_multitrack(
    bot_state: Data,
    guild_id: GuildId,
    multitrack_state: MultiTrackPlaybackState,
) {
    if multitrack_state.tracks.is_empty() {
        return;
    }
    tracing::info!("Restoring {} track(s)", multitrack_state.tracks.len());

    if let Err(e) = bot_state.load(guild_id).await {
        tracing::error!("Failed to load the guild to restore its tracks: {e:#}");
        return;
    }
    let Some(radio) = bot_state.radio(guild_id) else {
        return;
    };
    let manager = radio.track_manager;

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
