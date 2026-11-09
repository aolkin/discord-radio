use crate::audio::connection::connect_voice_channel;
use crate::audio::processing::processor::ProcessorRef;
use crate::commands::utils;
use crate::commands::utils::{Context, require_guild};
use anyhow::Context as _;
use serenity::all::{ChannelId, ChannelType, GuildId};

/// Join a voice channel to broadcast audio
#[poise::command(slash_command, guild_only)]
#[tracing::instrument(skip_all, fields(guild_id = ctx.guild_id().map(|id| id.get())))]
pub async fn join(
    ctx: Context<'_>,
    #[description = "Voice channel to join and broadcast audio"] channel: ChannelId,
) -> anyhow::Result<()> {
    let guild_id = require_guild(ctx)?;
    let user_id = ctx.author().id;

    tracing::info!(
        "User {} executed voice join for channel {}",
        user_id,
        channel,
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

    join_voice_channel_helper(ctx, guild_id, channel)
        .await
        .context("Failed to join the voice channel")?;

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
#[poise::command(slash_command, guild_only)]
#[tracing::instrument(skip_all, fields(guild_id = ctx.guild_id().map(|id| id.get())))]
pub async fn leave(ctx: Context<'_>) -> anyhow::Result<()> {
    let guild_id = require_guild(ctx)?;
    let user_id = ctx.author().id;

    tracing::info!("User {} executed voice leave", user_id,);

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
            if let Some(dj_manager) = ctx.data().existing_dj_manager(guild_id) {
                dj_manager.stop().await;
            }
            ctx.say("Left voice channel").await?;
        }
    } else {
        ctx.say("Not in a voice channel").await?;
    }

    Ok(())
}

async fn join_voice_channel_helper(
    ctx: Context<'_>,
    guild_id: GuildId,
    channel_id: ChannelId,
) -> anyhow::Result<ProcessorRef> {
    let manager = songbird::get(ctx.serenity_context())
        .await
        .expect("Songbird Voice client placed in at initialisation.")
        .clone();

    let processor = connect_voice_channel(&manager, guild_id, channel_id, ctx.data()).await?;
    tracing::info!("Joined voice channel");
    ctx.data()
        .dj_manager(guild_id)
        .start()
        .await
        .map_err(|e| e.context("Failed to start DJ"))?;
    Ok(processor)
}

/// Join or leave a voice channel
#[poise::command(
    slash_command,
    guild_only,
    default_member_permissions = "ADMINISTRATOR",
    subcommands("join", "leave")
)]
pub async fn voice(_ctx: Context<'_>) -> anyhow::Result<()> {
    Ok(())
}
