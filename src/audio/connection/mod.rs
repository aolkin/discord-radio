use crate::audio::processing::processor::ProcessorRef;
use crate::state::Data;
use raw_adapter::ProcessedAudioAdapter;
use serenity::model::id::GuildId;
use songbird::Call;
use std::sync::Arc;
use tokio::sync::Mutex;

mod raw_adapter;

pub async fn setup_voice_connection(
    handle_lock: Arc<Mutex<Call>>,
    guild_id: GuildId,
    bot_state: Data,
) -> ProcessorRef {
    {
        let mut voice_connections = bot_state.voice_connections.write().await;
        voice_connections.insert(guild_id, handle_lock.clone());
    }

    let mut call = handle_lock.lock().await;
    let event_handler = crate::handlers::voice::ConnectionEventHandler { data: bot_state };

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

    // Get or create DSP processor with default "clear" profile
    let processor = bot_state.audio_processor(guild_id).await;

    // Start the adapter with the processor to connect it to Songbird
    let adapter = ProcessedAudioAdapter::new(processor.clone());
    adapter.start(handle_lock.clone()).await;

    tracing::info!("Initialized audio DSP processor for guild {}", guild_id);

    processor
}
