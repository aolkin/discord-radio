use songbird::input::Input;
use std::path::Path;
use std::sync::Arc;
use tokio::time::{Duration, sleep};
use tokio_util::sync::CancellationToken;

async fn create_audio_source(
    audio_file_path: &str,
) -> Result<Input, Box<dyn std::error::Error + Send + Sync>> {
    if !Path::new(audio_file_path).exists() {
        return Err(format!("Audio file not found: {}", audio_file_path).into());
    }

    let path = audio_file_path.to_string();
    let source = Input::from(songbird::input::File::new(path));
    Ok(source)
}

pub fn message_to_hex_sequence(message: &str) -> Vec<char> {
    let mut hex_chars = Vec::new();

    for byte in message.bytes() {
        let hex = format!("{:02X}", byte);
        hex_chars.extend(hex.chars());
    }

    hex_chars
}

pub async fn play_hex_sequence_looping(
    call_lock: Arc<tokio::sync::Mutex<songbird::Call>>,
    hex_audio_dir: String,
    message: String,
    cancel_token: CancellationToken,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let hex_chars = message_to_hex_sequence(&message);

    tracing::info!(
        "Starting looping hex sequence for message: {} -> {:?}",
        message,
        hex_chars
    );

    loop {
        for (i, hex_char) in hex_chars.iter().enumerate() {
            if cancel_token.is_cancelled() {
                tracing::info!("Message playback cancelled");
                return Ok(());
            }

            let audio_path = format!("{}/hex_{}.mp3", hex_audio_dir, hex_char);

            if !Path::new(&audio_path).exists() {
                tracing::warn!("Audio file not found: {}", audio_path);
                continue;
            }

            let source = create_audio_source(&audio_path).await?;

            {
                let mut call = call_lock.lock().await;
                let handle = call.play_input(source);
                drop(call);

                let _ = handle.get_info().await?;

                while !handle.get_info().await?.playing.is_done() {
                    if cancel_token.is_cancelled() {
                        let _ = handle.stop();
                        tracing::info!("Message playback cancelled");
                        return Ok(());
                    }
                    sleep(Duration::from_millis(50)).await;
                }
            }

            if (i + 1) % 2 == 0 && i + 1 < hex_chars.len() {
                tokio::select! {
                    _ = sleep(Duration::from_millis(500)) => {}
                    _ = cancel_token.cancelled() => {
                        tracing::info!("Message playback cancelled");
                        return Ok(());
                    }
                }
            } else if i + 1 < hex_chars.len() {
                tokio::select! {
                    _ = sleep(Duration::from_millis(200)) => {}
                    _ = cancel_token.cancelled() => {
                        tracing::info!("Message playback cancelled");
                        return Ok(());
                    }
                }
            }
        }

        tracing::info!("Hex sequence iteration complete, pausing before repeat");

        tokio::select! {
            _ = sleep(Duration::from_secs(3)) => {}
            _ = cancel_token.cancelled() => {
                tracing::info!("Message playback cancelled");
                return Ok(());
            }
        }
    }
}
