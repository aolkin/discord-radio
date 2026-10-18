use crate::audio::processing::processor::ProcessorRef;
use crate::state::Data;
use anyhow::Context;
use raw_adapter::ProcessedAudioAdapter;
use serenity::model::id::{ChannelId, GuildId};
use songbird::Songbird;

mod raw_adapter;

pub async fn connect_voice_channel(
    manager: &Songbird,
    guild_id: GuildId,
    channel_id: ChannelId,
    bot_state: Data,
) -> anyhow::Result<ProcessorRef> {
    // Songbird keeps a Call, with its events and tracks, across leave and disconnect.
    let call_exists = manager.get(guild_id).is_some();
    let handle_lock = manager.get_or_insert(guild_id);

    let processor = bot_state.audio_processor(guild_id).await;

    if !call_exists {
        let mut call = handle_lock.lock().await;
        let event_handler = crate::handlers::voice::ConnectionEventHandler { data: bot_state };

        for event in [
            songbird::events::CoreEvent::DriverConnect,
            songbird::events::CoreEvent::DriverDisconnect,
            songbird::events::CoreEvent::DriverReconnect,
            songbird::events::CoreEvent::ClientDisconnect,
        ] {
            call.add_global_event(songbird::Event::Core(event), event_handler.clone());
        }

        drop(call);

        let adapter = ProcessedAudioAdapter::new(processor.clone());
        adapter.start(handle_lock.clone()).await;

        tracing::info!("Initialized audio DSP processor for guild {}", guild_id);
    }

    manager
        .join(guild_id, channel_id)
        .await
        .context("joining voice channel")?;

    {
        let mut voice_connections = bot_state.voice_connections.write().await;
        voice_connections.insert(guild_id, handle_lock);
    }

    Ok(processor)
}
