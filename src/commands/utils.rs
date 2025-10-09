use crate::state::Data;
use serenity::all::{Channel, ChannelId};

pub type Error = Box<dyn std::error::Error + Send + Sync>;
pub type Context<'a> = poise::Context<'a, Data, Error>;

pub async fn get_channel_details(ctx: Context<'_>, channel: ChannelId) -> Result<Channel, Error> {
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
            Err(Box::new(e))
        }
    }
}
