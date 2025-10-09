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
                    handle_lock,
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

    Ok(())
}
