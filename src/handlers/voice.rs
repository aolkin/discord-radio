use serenity::model::id::GuildId;
use songbird::{Event, EventContext, EventHandler};

#[derive(Clone)]
pub struct ConnectionEventHandler;

#[async_trait::async_trait]
impl EventHandler for ConnectionEventHandler {
    async fn act(&self, ctx: &EventContext<'_>) -> Option<Event> {
        use EventContext as Ctx;

        match ctx {
            Ctx::DriverConnect(data) => {
                tracing::info!(
                    "Bot connected to voice channel in guild {:?}, channel {:?}",
                    data.guild_id,
                    data.channel_id
                );
            }
            Ctx::DriverDisconnect(data) => {
                tracing::info!(
                    "Bot disconnected from voice channel in guild {:?}, channel {:?}",
                    data.guild_id,
                    data.channel_id
                );

                let guild_id = GuildId::new(data.guild_id.0.into());

                tracing::info!(
                    "Voice disconnected for guild {}, audio state preserved",
                    guild_id
                );
            }
            Ctx::DriverReconnect(data) => {
                tracing::info!(
                    "Bot reconnected to voice channel in guild {:?}, channel {:?}",
                    data.guild_id,
                    data.channel_id
                );
            }
            _ => {}
        }

        None
    }
}
