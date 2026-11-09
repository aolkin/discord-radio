use crate::commands::utils::{Context, require_guild};
use anyhow::Context as _;
use poise::serenity_prelude as serenity;

pub fn build_message(
    message: Option<String>,
    title: Option<String>,
    description: Option<String>,
) -> serenity::all::CreateMessage {
    let mut create_message = serenity::all::CreateMessage::new();

    if let Some(msg) = message {
        create_message = create_message.content(msg);
    }

    if title.is_some() || description.is_some() {
        let mut embed = serenity::all::CreateEmbed::new();

        if let Some(t) = title {
            embed = embed.title(t);
        }

        if let Some(d) = description {
            embed = embed.description(d);
        }

        create_message = create_message.embed(embed);
    }

    create_message
}

/// Register a channel for sending messages from the web portal
#[poise::command(slash_command, guild_only)]
#[tracing::instrument(skip_all, fields(guild_id = ctx.guild_id().map(|id| id.get())))]
pub async fn register(
    ctx: Context<'_>,
    #[description = "Channel to register"] channel: serenity::Channel,
) -> anyhow::Result<()> {
    let user_id = ctx.author().id;
    let guild_id = require_guild(ctx)?;

    tracing::info!("User {} executed message register", user_id,);

    ctx.defer_ephemeral().await?;

    // Get channel info
    let channel_id = channel.id();
    let channel_name = match &channel {
        serenity::Channel::Guild(gc) => gc.name.clone(),
        _ => channel_id.to_string(),
    };

    let channel_type = match &channel {
        serenity::Channel::Guild(gc) => format!("{:?}", gc.kind),
        serenity::Channel::Private(_) => "Private".to_string(),
        _ => "Unknown".to_string(),
    };

    // Create registered channel entry
    let registered_channel = crate::persistence::RegisteredChannel {
        channel_id,
        guild_id,
        name: channel_name.clone(),
        channel_type: channel_type.clone(),
    };

    // Save to persistent storage
    ctx.data()
        .state_store
        .save_registered_channel(&registered_channel)
        .await
        .context("Failed to register channel")?;

    ctx.say(format!(
        "Channel '{}' (type: {}) has been registered for web portal messaging",
        channel_name, channel_type
    ))
    .await?;

    Ok(())
}

/// Send a message with optional embed to the current channel
#[poise::command(slash_command, guild_only)]
#[tracing::instrument(skip_all, fields(guild_id = ctx.guild_id().map(|id| id.get())))]
pub async fn speak(
    ctx: Context<'_>,
    #[description = "Message to send"] message: Option<String>,
    #[description = "Title for embed"] title: Option<String>,
    #[description = "Description for embed"] description: Option<String>,
) -> anyhow::Result<()> {
    let user_id = ctx.author().id;

    tracing::info!("User {} executed message speak", user_id);

    ctx.defer_ephemeral().await?;

    let channel_id = ctx.channel_id();

    let create_message = build_message(message, title, description);

    channel_id
        .send_message(crate::http(), create_message)
        .await
        .context("Failed to send message")?;

    ctx.say("Message sent").await?;

    Ok(())
}

/// Register channels and send messages
#[poise::command(
    slash_command,
    guild_only,
    default_member_permissions = "ADMINISTRATOR",
    subcommands("register", "speak")
)]
pub async fn message(_ctx: Context<'_>) -> anyhow::Result<()> {
    Ok(())
}
