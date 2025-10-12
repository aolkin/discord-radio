use crate::commands::utils::{Context, Error};
use poise::serenity_prelude as serenity;

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

    let activity = match activity_type.as_deref() {
        Some("listening") => serenity::ActivityData::listening(status.clone()),
        Some("playing") => serenity::ActivityData::playing(status.clone()),
        Some("streaming") => {
            serenity::ActivityData::streaming(status.clone(), "https://twitch.tv/")
                .unwrap_or_else(|_| serenity::ActivityData::custom(status.clone()))
        }
        Some("custom") | None => serenity::ActivityData::custom(status.clone()),
        Some(invalid) => {
            ctx.say(format!(
                "Invalid activity type '{}'. Valid types: listening, playing, streaming, custom",
                invalid
            ))
            .await?;
            return Ok(());
        }
    };

    ctx.serenity_context().set_activity(Some(activity));

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
