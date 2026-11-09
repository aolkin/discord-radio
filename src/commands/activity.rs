use crate::activity::ActivityType;
use crate::commands::utils::Context;

/// Set the bot's custom status
#[poise::command(
    slash_command,
    guild_only,
    default_member_permissions = "ADMINISTRATOR"
)]
#[tracing::instrument(skip_all, fields(guild_id = ctx.guild_id().map(|id| id.get())))]
pub async fn activity(
    ctx: Context<'_>,
    #[description = "Custom status text"] status: String,
    #[description = "Activity type (defaults to custom)"]
    #[rename = "type"]
    activity_type: Option<ActivityType>,
) -> anyhow::Result<()> {
    let user_id = ctx.author().id;

    tracing::info!(
        "User {} executed activity with type {:?}",
        user_id,
        activity_type
    );

    ctx.defer_ephemeral().await?;

    let activity_type = activity_type.unwrap_or(ActivityType::Custom);

    ctx.data()
        .activity_manager
        .set_activity(activity_type, status.clone())
        .await;

    ctx.say(format!("Status set to: {} ({})", status, activity_type))
        .await?;

    Ok(())
}
