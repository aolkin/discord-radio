use songbird::{
    Event, EventContext, EventHandler as VoiceEventHandler, TrackEvent, input::Input,
    tracks::TrackHandle,
};
use std::path::Path;
use std::sync::Arc;

pub async fn create_audio_source(
    audio_file_path: &str,
) -> Result<Input, Box<dyn std::error::Error + Send + Sync>> {
    if !Path::new(audio_file_path).exists() {
        return Err(format!("Audio file not found: {}", audio_file_path).into());
    }

    let path = audio_file_path.to_string();
    let source = Input::from(songbird::input::File::new(path));
    Ok(source)
}

pub struct LoopingHandler {
    pub call_handle: Arc<tokio::sync::Mutex<songbird::Call>>,
    pub audio_file_path: String,
}

#[async_trait::async_trait]
impl VoiceEventHandler for LoopingHandler {
    async fn act(&self, ctx: &EventContext<'_>) -> Option<Event> {
        use EventContext as Ctx;

        if let Ctx::Track(track_list) = ctx {
            for (state, _handle) in *track_list {
                if state.playing == songbird::tracks::PlayMode::Stop {
                    tracing::info!("Track ended, restarting audio loop");

                    if let Ok(source) = create_audio_source(&self.audio_file_path).await {
                        let mut call = self.call_handle.lock().await;
                        let _new_handle = call.play_input(source);
                    }
                    break;
                }
            }
        }

        None
    }
}

pub async fn start_audio_playback(
    call_lock: Arc<tokio::sync::Mutex<songbird::Call>>,
    audio_file_path: &str,
) -> Result<TrackHandle, Box<dyn std::error::Error + Send + Sync>> {
    let mut call = call_lock.lock().await;

    let source = create_audio_source(audio_file_path).await?;
    let handle = call.play_input(source);

    let looping_handler = LoopingHandler {
        call_handle: call_lock.clone(),
        audio_file_path: audio_file_path.to_string(),
    };

    handle
        .add_event(Event::Track(TrackEvent::End), looping_handler)
        .map_err(|e| format!("Failed to add event handler: {}", e))?;

    tracing::info!("Started audio playback with looping");

    Ok(handle)
}
