use crate::commands::utils::{Context, Error};
use poise::serenity_prelude as serenity;

/// Register a channel for sending messages from the web portal
#[poise::command(
    slash_command,
    guild_only,
    default_member_permissions = "ADMINISTRATOR"
)]
pub async fn register_channel(
    ctx: Context<'_>,
    #[description = "Channel to register"] channel: serenity::Channel,
) -> Result<(), Error> {
    let user_id = ctx.author().id;
    let guild_id = ctx
        .guild_id()
        .ok_or("This command can only be used in a server")?;

    tracing::info!(
        "User {} executed register_channel in guild {}",
        user_id,
        guild_id
    );

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
    if let Err(e) = ctx
        .data()
        .state_store
        .save_registered_channel(&registered_channel)
        .await
    {
        tracing::error!("Failed to save registered channel: {}", e);
        ctx.say("Failed to register channel").await?;
        return Ok(());
    }

    ctx.say(format!(
        "Channel '{}' (type: {}) has been registered for web portal messaging",
        channel_name, channel_type
    ))
    .await?;

    Ok(())
}

/// Set the bot's custom status
#[poise::command(
    slash_command,
    guild_only,
    default_member_permissions = "ADMINISTRATOR"
)]
pub async fn set_status(
    ctx: Context<'_>,
    #[description = "Custom status text"] status: String,
    #[description = "Activity type (listening, playing, streaming, or custom)"]
    #[rename = "type"]
    activity_type: Option<String>,
) -> Result<(), Error> {
    let user_id = ctx.author().id;
    let guild_id = ctx
        .guild_id()
        .ok_or("This command can only be used in a server")?;

    tracing::info!(
        "User {} executed set_status in guild {} with type {:?}",
        user_id,
        guild_id,
        activity_type
    );

    ctx.defer_ephemeral().await?;

    // Parse the activity type
    let activity_type_enum = match activity_type.as_deref() {
        Some("listening") => crate::voice_status::ActivityType::Listening,
        Some("playing") => crate::voice_status::ActivityType::Playing,
        Some("streaming") => crate::voice_status::ActivityType::Streaming,
        Some("custom") | None => crate::voice_status::ActivityType::Custom,
        Some(invalid) => {
            ctx.say(format!(
                "Invalid activity type '{}'. Valid types: listening, playing, streaming, custom",
                invalid
            ))
            .await?;
            return Ok(());
        }
    };

    // Clear the stack and set the new activity
    ctx.data()
        .activity_manager
        .set_activity(activity_type_enum, status.clone())
        .await;

    let type_str = activity_type.as_deref().unwrap_or("custom");
    ctx.say(format!("Status set to: {} ({})", status, type_str))
        .await?;

    Ok(())
}

/// Send a message with optional embed to the current channel
#[poise::command(
    slash_command,
    guild_only,
    default_member_permissions = "ADMINISTRATOR"
)]
pub async fn speak(
    ctx: Context<'_>,
    #[description = "Message to send"] message: Option<String>,
    #[description = "Title for embed"] title: Option<String>,
    #[description = "Description for embed"] description: Option<String>,
) -> Result<(), Error> {
    let user_id = ctx.author().id;
    let guild_id = ctx
        .guild_id()
        .ok_or("This command can only be used in a server")?;

    tracing::info!("User {} executed speak in guild {}", user_id, guild_id);

    ctx.defer_ephemeral().await?;

    let channel_id = ctx.channel_id();

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

    if let Err(e) = channel_id.send_message(ctx.http(), create_message).await {
        ctx.say(format!("Failed to send message: {}", e)).await?;
        return Ok(());
    }

    ctx.say("Message sent").await?;

    Ok(())
}
