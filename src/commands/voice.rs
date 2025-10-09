use crate::commands::utils;
use crate::commands::utils::{Context, Error};
use serenity::all::{ChannelId, ChannelType};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

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

    let manager = songbird::get(ctx.serenity_context())
        .await
        .expect("Songbird Voice client placed in at initialisation.")
        .clone();

    match manager.join(guild_id, channel).await {
        Ok(handle_lock) => {
            tracing::info!("Joined voice channel");

            if let Err(e) = crate::audio::connection::setup_voice_connection(
                handle_lock,
                guild_id,
                ctx.data().clone(),
            )
            .await
            {
                ctx.say(format!("Failed to setup voice connection: {}", e))
                    .await?;
                return Ok(());
            }

            let _ = ctx
                .data()
                .state_store
                .save_voice_channel(guild_id, channel)
                .await;

            ctx.say(format!("Joined voice channel <#{}>", channel))
                .await?;
        }
        Err(e) => {
            ctx.say(format!("Failed to join the voice channel: {}", e))
                .await?;
        }
    }

    Ok(())
}

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
            let mut voice_connections = ctx.data().voice_connections.write().await;
            voice_connections.remove(&guild_id);

            let _ = ctx.data().state_store.remove_voice_channel(guild_id).await;

            // Stop any playing tracks
            let mut track_handles = ctx.data().track_handles.write().await;
            if let Some(handle) = track_handles.remove(&guild_id) {
                let _ = handle.stop();
            }

            // Cancel any ongoing message playback
            let mut message_playback_tokens = ctx.data().message_playback_tokens.write().await;
            if let Some(cancel_token) = message_playback_tokens.remove(&guild_id) {
                cancel_token.cancel();
            }

            ctx.say("Left voice channel").await?;
        }
    } else {
        ctx.say("Not in a voice channel").await?;
    }

    Ok(())
}

#[poise::command(
    slash_command,
    guild_only,
    default_member_permissions = "ADMINISTRATOR"
)]
pub async fn play_message(
    ctx: Context<'_>,
    #[description = "Message to convert to hex and play"] message: String,
    #[description = "Voice channel to join (optional)"] channel: Option<ChannelId>,
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

        let manager = songbird::get(ctx.serenity_context())
            .await
            .expect("Songbird Voice client placed in at initialisation.")
            .clone();

        match manager.join(guild_id, channel_id).await {
            Ok(handle_lock) => {
                tracing::info!("Joined voice channel for play_message");
                let mut voice_connections = ctx.data().voice_connections.write().await;
                voice_connections.insert(guild_id, handle_lock.clone());

                let mut call = handle_lock.lock().await;
                let event_handler = crate::handlers::voice::ConnectionEventHandler {
                    data: ctx.data().clone(),
                };

                call.add_global_event(
                    songbird::Event::Core(songbird::events::CoreEvent::DriverConnect),
                    event_handler.clone(),
                );
                call.add_global_event(
                    songbird::Event::Core(songbird::events::CoreEvent::DriverDisconnect),
                    event_handler.clone(),
                );
                call.add_global_event(
                    songbird::Event::Core(songbird::events::CoreEvent::DriverReconnect),
                    event_handler.clone(),
                );
                call.add_global_event(
                    songbird::Event::Core(songbird::events::CoreEvent::ClientDisconnect),
                    event_handler,
                );

                drop(call);
            }
            Err(e) => {
                reply
                    .edit(
                        ctx,
                        poise::CreateReply::default()
                            .content(format!("Failed to join the voice channel: {}", e))
                            .ephemeral(true),
                    )
                    .await?;
                return Ok(());
            }
        }
    }

    let voice_connections = ctx.data().voice_connections.read().await;
    let call_lock = match voice_connections.get(&guild_id) {
        Some(lock) => Arc::clone(lock),
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
    drop(voice_connections);

    let mut track_handles = ctx.data().track_handles.write().await;
    if let Some(handle) = track_handles.get(&guild_id) {
        let _ = handle.stop();
    }
    track_handles.remove(&guild_id);
    drop(track_handles);

    let mut message_playback_tokens = ctx.data().message_playback_tokens.write().await;
    if let Some(old_token) = message_playback_tokens.get(&guild_id) {
        old_token.cancel();
    }

    let cancel_token = CancellationToken::new();
    message_playback_tokens.insert(guild_id, cancel_token.clone());
    drop(message_playback_tokens);

    let hex_audio_dir = ctx.data().hex_audio_dir.clone();
    let message_clone = message.clone();

    tokio::spawn(async move {
        if let Err(e) = crate::audio::manager::play_hex_sequence_looping(
            call_lock,
            hex_audio_dir,
            message_clone,
            cancel_token,
        )
        .await
        {
            tracing::error!("Error in message playback loop: {}", e);
        }
    });

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

    let mut message_playback_tokens = ctx.data().message_playback_tokens.write().await;

    if let Some(cancel_token) = message_playback_tokens.remove(&guild_id) {
        cancel_token.cancel();
        ctx.say("Message playback stopped").await?;
    } else {
        ctx.say("No message is currently playing").await?;
    }

    Ok(())
}
