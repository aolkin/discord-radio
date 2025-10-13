use crate::state::Data;
use tokio::signal;
use tracing::{error, info};

pub async fn setup_shutdown_handler(data: Data) {
    let mut shutdown_rx = data.shutdown_tx.subscribe();

    tokio::spawn(async move {
        let shutdown_reason = tokio::select! {
            result = wait_for_shutdown_signal() => {
                if let Err(e) = result {
                    error!("Error waiting for shutdown signal: {}", e);
                }
                "OS signal (SIGTERM/SIGINT/Ctrl+C)".to_string()
            }
            reason = shutdown_rx.recv() => {
                match reason {
                    Ok(r) => {
                        info!("Emergency shutdown requested: {}", r);
                        r
                    }
                    Err(e) => {
                        error!("Error receiving shutdown signal: {}", e);
                        "Emergency shutdown (channel error)".to_string()
                    }
                }
            }
        };

        info!("Shutdown triggered by: {}", shutdown_reason);
        info!("Cleaning up...");

        // Abort all hex playback tasks
        let hex_playback_tasks = data.hex_playback_tasks.write().await;
        for (guild_id, handle) in hex_playback_tasks.iter() {
            info!("Aborting hex playback task for guild {}", guild_id);
            handle.abort();
        }
        drop(hex_playback_tasks);

        // Stop all audio tracks
        let track_managers = data.track_managers.read().await;
        for (guild_id, manager_arc) in track_managers.iter() {
            info!("Stopping all tracks for guild {}", guild_id);
            let mut manager = manager_arc.lock().await;
            if let Err(e) = manager.stop_all_tracks(0.0, false).await {
                error!("Failed to stop tracks for guild {}: {}", guild_id, e);
            }
        }
        drop(track_managers);

        // Give fade tasks time to complete
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

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
