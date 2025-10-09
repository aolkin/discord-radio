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

                let mut track_handles = self.data.track_handles.write().await;
                if let Some(handle) = track_handles.remove(&guild_id) {
                    let _ = handle.stop();
                }

                tracing::info!("Cleaned up state for disconnected guild {}", guild_id);
            }
            Ctx::DriverReconnect(data) => {
                tracing::info!(
                    "Bot reconnected to voice channel in guild {:?}, channel {:?}",
                    data.guild_id,
                    data.channel_id
                );

                let guild_id = GuildId::new(data.guild_id.0.into());
                if let Some(call_lock) = self.data.voice_connections.read().await.get(&guild_id) {
                    tracing::info!(
                        "Resuming audio playback for guild {} after reconnect",
                        guild_id
                    );

                    if let Ok(track_handle) = crate::audio::manager::start_audio_playback(
                        call_lock.clone(),
                        &self.data.audio_file_path,
                    )
                    .await
                    {
                        let mut track_handles = self.data.track_handles.write().await;
                        track_handles.insert(guild_id, track_handle);
                    }
                }
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
