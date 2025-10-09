use crate::commands::utils;
use crate::commands::utils::{Context, Error};
use serenity::all::{ChannelId, ChannelType};

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
            let mut voice_connections = ctx.data().voice_connections.write().await;
            voice_connections.insert(guild_id, handle_lock.clone());

            // Register connection event handlers
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

            ctx.say(format!(
                "Joined voice channel <#{}> and started broadcasting",
                channel
            ))
            .await?;

            // Start audio playback with looping
            if let Ok(track_handle) = crate::audio::manager::start_audio_playback(
                handle_lock,
                &ctx.data().audio_file_path,
            )
            .await
            {
                let mut track_handles = ctx.data().track_handles.write().await;
                track_handles.insert(guild_id, track_handle);
            }
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

            // Stop any playing tracks
            let mut track_handles = ctx.data().track_handles.write().await;
            if let Some(handle) = track_handles.remove(&guild_id) {
                let _ = handle.stop();
            }

            ctx.say("Left voice channel and stopped broadcasting")
                .await?;
        }
    } else {
        ctx.say("Not in a voice channel").await?;
    }

    Ok(())
}
