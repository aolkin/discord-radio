use crate::audio::raw_adapter::ProcessedAudioAdapter;
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

    // Get or create DSP processor with default "clear" profile
    let processor = crate::audio::tracks::get_or_create_audio_processor(&bot_state, guild_id).await;

    // Start the adapter with the processor to connect it to Songbird
    let adapter = ProcessedAudioAdapter::new(processor.clone());
    adapter.start(handle_lock.clone()).await;

    // Restore saved profile state if available
    if let Ok(profile_states) = bot_state.state_store.load_profile_states().await
        && let Some(profile_state) = profile_states.get(&guild_id)
    {
        let mut proc = processor.write().await;

        if profile_state.bypass {
            proc.set_bypass(true);
            tracing::info!("Restored bypass state for guild {}", guild_id);
        } else {
            // Load the saved profile from ProfileManager
            if let Some(saved_profile) = bot_state
                .profile_manager
                .get_profile(&profile_state.profile_name)
            {
                proc.set_profile_immediate(saved_profile.clone());
                tracing::info!(
                    "Restored profile '{}' for guild {}",
                    profile_state.profile_name,
                    guild_id
                );
            } else {
                tracing::warn!(
                    "Saved profile '{}' not found, using default",
                    profile_state.profile_name
                );
            }
        }
    }

    tracing::info!("Initialized audio DSP processor for guild {}", guild_id);

    Ok(())
}
