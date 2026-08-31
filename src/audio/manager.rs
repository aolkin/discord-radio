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
        let relative_filename = format!("audio/hex/hex_{}.wav", hex_char);
        match bot_state.file_resolver.resolve(&relative_filename).await {
            Ok(path) if Path::new(&path).exists() => {}
            _ => {
                tracing::warn!("Audio file not found: {}", relative_filename);
                let next_position = if position + 1 >= hex_chars.len() {
                    0
                } else {
                    position + 1
                };
                playback_state.write().await.current_position = next_position;
                continue;
            }
        }

        let notify = Arc::new(tokio::sync::Notify::new());
        let notify_clone = notify.clone();

        {
            let mut manager = manager_arc.lock().await;

            // Remove existing hex track if it exists (shouldn't happen, but guard against it)
            if manager.has_track(HEX_PLAYBACK_TRACK_NAME) {
                manager.remove_track(HEX_PLAYBACK_TRACK_NAME).await;
            }

            let callback = Arc::new(move || {
                notify_clone.notify_one();
            });

            if let Err(e) = manager
                .start_track_with_callback(
                    StartTrackArgs {
                        name: HEX_PLAYBACK_TRACK_NAME.to_string(),
                        filename: relative_filename,
                        volume,
                        fade_time: 0.0,
                        loops: false,
                        start_position: None,
                        persist: true,
                    },
                    Some(callback),
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

        let completed_loop = next_position == 0 && position + 1 >= hex_chars.len();

        let current_state = playback_state.read().await.clone();
        if current_state.message.as_ref() == Some(&message) {
            let mut state = playback_state.write().await;
            state.current_position = next_position;

            if completed_loop {
                state.current_loop += 1;
                tracing::info!(
                    "Hex message '{}' completed loop {} (target: {:?}) in guild {}",
                    message,
                    state.current_loop,
                    state.target_loops,
                    guild_id
                );

                if let Some(target) = state.target_loops
                    && state.current_loop >= target
                {
                    tracing::info!(
                        "Hex message '{}' completed {} loops in guild {}, stopping playback",
                        message,
                        state.current_loop,
                        guild_id
                    );
                    state.message = None;
                    state.current_position = 0;
                    state.current_loop = 0;

                    let state_store = bot_state.state_store.clone();
                    let guild_id_copy = guild_id;
                    tokio::spawn(async move {
                        if let Err(e) = state_store.remove_message_playback(guild_id_copy).await {
                            tracing::warn!("Failed to remove message playback state: {}", e);
                        }
                    });

                    continue;
                }
            }

            let persist_state = MessagePlaybackState {
                message: message.clone(),
                current_position: next_position,
                current_loop: state.current_loop,
                target_loops: state.target_loops,
            };
            drop(state);

            let state_store = bot_state.state_store.clone();
            let guild_id_copy = guild_id;
            tokio::spawn(async move {
                if let Err(e) = state_store
                    .save_message_playback(guild_id_copy, &persist_state)
                    .await
                {
                    tracing::warn!(
                        "Failed to save message playback progress for message '{}': {}",
                        persist_state.message,
                        e
                    );
                }
            });
        }

        let delay = if (position + 1) % 2 == 0 {
            Duration::from_millis(800)
        } else if position + 1 >= hex_chars.len() {
            Duration::from_secs(8)
        } else {
            Duration::from_millis(50)
        };

        sleep(delay).await;
    }
}
