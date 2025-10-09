use crate::state::Data;
use tokio::signal;
use tracing::{error, info};

pub async fn setup_shutdown_handler(data: Data) {
    tokio::spawn(async move {
        if let Err(e) = wait_for_shutdown_signal().await {
            error!("Error waiting for shutdown signal: {}", e);
        }

        info!("Shutdown signal received, cleaning up...");

        // Stop all audio tracks
        let track_handles = data.track_handles.read().await;
        for (guild_id, handle) in track_handles.iter() {
            info!("Stopping audio track for guild {}", guild_id);
            if let Err(e) = handle.stop() {
                error!("Failed to stop track for guild {}: {}", guild_id, e);
            }
        }
        drop(track_handles);

        // Clear voice connections (they will be cleaned up automatically)
        let voice_connections = data.voice_connections.read().await;
        info!(
            "Cleaning up {} voice connection(s)",
            voice_connections.len()
        );

        info!("Cleanup completed");
        std::process::exit(0);
    });
}

async fn wait_for_shutdown_signal() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    #[cfg(unix)]
    {
        use signal::unix::{SignalKind, signal};

        let mut sigterm = signal(SignalKind::terminate())?;
        let mut sigint = signal(SignalKind::interrupt())?;

        tokio::select! {
            _ = sigterm.recv() => {
                info!("Received SIGTERM");
            }
            _ = sigint.recv() => {
                info!("Received SIGINT");
            }
        }
    }

    #[cfg(windows)]
    {
        signal::ctrl_c().await?;
        info!("Received Ctrl+C");
    }

    Ok(())
}
