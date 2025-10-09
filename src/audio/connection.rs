use crate::state::Data;
use serenity::model::id::GuildId;
use songbird::Call;
use std::sync::Arc;
use tokio::sync::Mutex;

pub async fn setup_voice_connection(
    handle_lock: Arc<Mutex<Call>>,
    guild_id: GuildId,
    bot_state: Data,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    {
        let mut voice_connections = bot_state.voice_connections.write().await;
        voice_connections.insert(guild_id, handle_lock.clone());
    }

    let mut call = handle_lock.lock().await;
    let event_handler = crate::handlers::voice::ConnectionEventHandler {
        data: bot_state.clone(),
    };

    call.add_global_event(
        songbird::Event::Core(songbird::events::CoreEvent::DriverConnect),
        event_handler.clone(),
    );
    call.add_global_event(
        songbird::Event::Core(songbird::events::CoreEvent::DriverDisconnect),
        event_handler.clone(),
    );
    call.add_global_event(
        songbird::Event::Core(songbird::events::CoreEvent::DriverReconnect),
        event_handler.clone(),
    );
    call.add_global_event(
        songbird::Event::Core(songbird::events::CoreEvent::ClientDisconnect),
        event_handler,
    );

    drop(call);

    Ok(())
}
