use songbird::{Event, EventContext, EventHandler};

#[derive(Clone)]
pub struct ConnectionEventHandler;

#[async_trait::async_trait]
impl EventHandler for ConnectionEventHandler {
    #[tracing::instrument(skip_all, fields(guild_id = tracing::field::Empty))]
    async fn act(&self, ctx: &EventContext<'_>) -> Option<Event> {
        use EventContext as Ctx;

        match ctx {
            Ctx::DriverConnect(data) => {
                tracing::Span::current().record("guild_id", data.guild_id.0.get());
                tracing::info!("Bot connected to voice channel {:?}", data.channel_id);
            }
            Ctx::DriverDisconnect(data) => {
                tracing::Span::current().record("guild_id", data.guild_id.0.get());
                tracing::info!("Bot disconnected from voice channel {:?}", data.channel_id);

                tracing::info!("Voice disconnected, audio state preserved");
            }
            Ctx::DriverReconnect(data) => {
                tracing::Span::current().record("guild_id", data.guild_id.0.get());
                tracing::info!("Bot reconnected to voice channel {:?}", data.channel_id);
            }
            _ => {}
        }

        None
    }
}
