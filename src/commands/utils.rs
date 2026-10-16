use crate::state::Data;
use anyhow::Context as _;
use serenity::all::{Channel, ChannelId, GuildId};

pub type Context<'a> = poise::Context<'a, Data, anyhow::Error>;

pub fn require_guild(ctx: Context<'_>) -> anyhow::Result<GuildId> {
    ctx.guild_id()
        .context("This command can only be used in a server")
}

pub async fn get_channel_details(ctx: Context<'_>, channel: ChannelId) -> anyhow::Result<Channel> {
    match channel.to_channel(ctx.serenity_context()).await {
        Ok(info) => {
            tracing::debug!("Channel info from API: {:?}", info);
            Ok(info)
        }
        Err(e) => {
            tracing::warn!(
                "Could not fetch channel info for channel {}: {:?}",
                channel,
                e
            );
            ctx.say("Cannot access channel!").await?;
            Err(e).with_context(|| format!("fetching channel {channel}"))
        }
    }
}
