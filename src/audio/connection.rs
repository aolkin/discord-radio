use crate::audio::profiles::{ProfileManager, SignalProfile};
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
    let (processor, task_handle) = adapter.spawn(handle_lock.clone()).await;

    // Store processor and task
    {
        let mut processors = bot_state.audio_processors.write().await;
        processors.insert(guild_id, processor.clone());
    }
    {
        let mut tasks = bot_state.audio_processor_tasks.write().await;
        tasks.insert(guild_id, task_handle);
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

fn load_default_profile(bot_state: &Data) -> Result<SignalProfile, Box<dyn std::error::Error + Send + Sync>> {
    let profiles_dir = bot_state.audio_profiles_dir();
    let mut manager = ProfileManager::new(&profiles_dir);
    manager.load_all()?;

    manager.get_profile("clear")
        .cloned()
        .ok_or_else(|| "Default 'clear' profile not found".into())
}
