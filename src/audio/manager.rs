use crate::audio::tracks::{StartTrackArgs, TrackManager};
use crate::persistence::MessagePlaybackState;
use crate::state::{Data, HexPlaybackState};
use serenity::model::id::GuildId;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use tokio::time::{Duration, sleep};

pub const HEX_PLAYBACK_TRACK_NAME: &str = "hex_playback";

pub fn message_to_hex_sequence(message: &str) -> Vec<char> {
    let mut hex_chars = Vec::new();

    for byte in message.bytes() {
        let hex = format!("{:02X}", byte);
        hex_chars.extend(hex.chars());
    }

    hex_chars
}

pub async fn hex_playback_task(
    guild_id: GuildId,
    manager_arc: Arc<Mutex<TrackManager>>,
    hex_audio_dir: String,
    playback_state: Arc<RwLock<HexPlaybackState>>,
    bot_state: Data,
) {
    tracing::info!(
        "Starting hex playback singleton task for guild {}",
        guild_id
    );

    loop {
        let state = playback_state.read().await.clone();

        let Some(message) = state.message else {
            sleep(Duration::from_millis(100)).await;
            continue;
        };

        let hex_chars = message_to_hex_sequence(&message);
        let position = state.current_position;
        let volume = state.volume;

        if position >= hex_chars.len() {
            sleep(Duration::from_millis(100)).await;
            continue;
        }

        let hex_char = hex_chars[position];
        let audio_path = format!("{}/hex_{}.wav", hex_audio_dir, hex_char);

        if !Path::new(&audio_path).exists() {
            tracing::warn!("Audio file not found: {}", audio_path);
            let next_position = if position + 1 >= hex_chars.len() {
                0
            } else {
                position + 1
            };
            playback_state.write().await.current_position = next_position;
            continue;
        }

        let notify = Arc::new(tokio::sync::Notify::new());
        let notify_clone = notify.clone();

        {
            let mut manager = manager_arc.lock().await;
            let event_handler = crate::audio::events::HexCharacterEndHandler::new(notify_clone);
            if let Err(e) = manager
                .start_track_with_custom_handler(
                    StartTrackArgs {
                        name: HEX_PLAYBACK_TRACK_NAME.to_string(),
                        filename: audio_path,
                        volume,
                        fade_time: 0.0,
                        loops: false,
                        start_position: None,
                    },
                    Some(event_handler),
                )
                .await
            {
                tracing::warn!("Failed to start hex track for message '{}': {}", message, e);
                sleep(Duration::from_millis(100)).await;
                continue;
            }
        }

        notify.notified().await;

        let next_position = if position + 1 >= hex_chars.len() {
            0
        } else {
            position + 1
        };

        let current_state = playback_state.read().await.clone();
        if current_state.message.as_ref() == Some(&message) {
            playback_state.write().await.current_position = next_position;

            let state = MessagePlaybackState {
                message: message.clone(),
                current_position: next_position,
            };
            let state_store = bot_state.state_store.clone();
            let guild_id_copy = guild_id;
            tokio::spawn(async move {
                if let Err(e) = state_store
                    .save_message_playback(guild_id_copy, &state)
                    .await
                {
                    tracing::warn!(
                        "Failed to save message playback progress for message '{}': {}",
                        state.message,
                        e
                    );
                }
            });
        }

        let delay = if (position + 1) % 2 == 0 {
            Duration::from_millis(800)
        } else if position + 1 >= hex_chars.len() {
            Duration::from_secs(3)
        } else {
            Duration::from_millis(50)
        };

        sleep(delay).await;
    }
}
