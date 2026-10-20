use songbird::{Event, EventContext, EventHandler};

#[derive(Clone)]
pub struct ConnectionEventHandler;

#[async_trait::async_trait]
impl EventHandler for ConnectionEventHandler {
    #[tracing::instrument(skip_all, fields(guild_id = tracing::field::Empty))]
    async fn act(&self, ctx: &EventContext<'_>) -> Option<Event> {
        use EventContext as Ctx;

        let (verb, guild_id, channel_id) = match ctx {
            Ctx::DriverConnect(data) => ("connected to", data.guild_id, data.channel_id),
            Ctx::DriverDisconnect(data) => ("disconnected from", data.guild_id, data.channel_id),
            Ctx::DriverReconnect(data) => ("reconnected to", data.guild_id, data.channel_id),
            _ => return None,
        };
        tracing::Span::current().record("guild_id", guild_id.0.get());
        tracing::info!("Bot {verb} voice channel {channel_id:?}");

        None
    }
}
