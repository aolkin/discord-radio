use crate::state::Data;
use serenity::model::id::{GuildId, UserId};
use songbird::{Event, EventContext, EventHandler};

#[derive(Clone)]
pub struct ConnectionEventHandler {
    pub data: Data,
}

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
                let mut voice_connections = self.data.voice_connections.write().await;
                voice_connections.remove(&guild_id);

                // Note: We no longer clean up TrackManager, audio_processor, hex playback, etc.
                // since they are now fully decoupled from voice connections.
                // DJ and other audio features can continue operating in the background.
                // They won't actually produce audio without Songbird consuming from the processor,
                // but their state machines and timers continue to run independently.

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
            Ctx::ClientDisconnect(data) => {
                let user_id = UserId::new(data.user_id.0);
                tracing::info!("User {} disconnected from voice channel", user_id);
            }
            _ => {}
        }

        None
    }
}
