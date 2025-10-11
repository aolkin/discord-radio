use crate::audio::profiles::SignalProfile;
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

    // Initialize DSP processor with default "clear" profile
    let initial_profile = load_default_profile(&bot_state)?;
    let adapter = ProcessedAudioAdapter::new(initial_profile);
    let processor = adapter.start(handle_lock.clone()).await;

    // Store processor
    {
        let mut processors = bot_state.audio_processors.write().await;
        processors.insert(guild_id, processor.clone());
    }

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

    // Link processor to TrackManager if it exists
    {
        let mut track_managers = bot_state.track_managers.write().await;
        if let Some(manager_arc) = track_managers.get_mut(&guild_id) {
            let mut manager = manager_arc.lock().await;
            manager.set_audio_processor(processor.clone());
        }
    }

    tracing::info!("Initialized audio DSP processor for guild {}", guild_id);

    Ok(())
}

fn load_default_profile(
    bot_state: &Data,
) -> Result<SignalProfile, Box<dyn std::error::Error + Send + Sync>> {
    bot_state
        .profile_manager
        .get_profile("clear")
        .cloned()
        .ok_or_else(|| "Default 'clear' profile not found".into())
}
