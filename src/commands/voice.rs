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

            ctx.say(format!(
                "Joined voice channel <#{}> and started broadcasting",
                channel
            ))
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

            ctx.say("Left voice channel and stopped broadcasting")
                .await?;
        }
    } else {
        ctx.say("Not in a voice channel").await?;
    }

    Ok(())
}
