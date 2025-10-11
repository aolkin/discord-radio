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

                let mut track_managers = self.data.track_managers.write().await;
                if let Some(manager_arc) = track_managers.remove(&guild_id) {
                    let mut manager = manager_arc.lock().await;
                    if let Err(e) = manager.stop_all_tracks(0.0, true).await {
                        tracing::warn!("Failed to stop tracks during disconnect: {}", e);
                    }
                }

                // Clean up DSP processor
                {
                    let mut processors = self.data.audio_processors.write().await;
                    processors.remove(&guild_id);
                }
                {
                    let mut tasks = self.data.audio_processor_tasks.write().await;
                    if let Some(task) = tasks.remove(&guild_id) {
                        task.abort();
                    }
                }

                let mut hex_playback_states = self.data.hex_playback_states.write().await;
                if let Some(state_arc) = hex_playback_states.remove(&guild_id) {
                    let mut state = state_arc.write().await;
                    *state = crate::state::HexPlaybackState::stopped();
                    drop(state);
                    drop(hex_playback_states);

                    if let Err(e) = self
                        .data
                        .state_store
                        .remove_message_playback(guild_id)
                        .await
                    {
                        tracing::warn!("Failed to remove message playback state: {}", e);
                    }
                }

                let mut hex_playback_tasks = self.data.hex_playback_tasks.write().await;
                if let Some(task) = hex_playback_tasks.remove(&guild_id) {
                    task.abort();
                }

                tracing::info!("Cleaned up state for disconnected guild {}", guild_id);
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
