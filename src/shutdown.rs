use crate::state::Data;
use tokio::signal;
use tracing::{error, info};

pub async fn setup_shutdown_handler(data: Data) {
    let mut shutdown_rx = data.shutdown_tx.subscribe();

    tokio::spawn(async move {
        let (clean_shutdown, shutdown_reason) = tokio::select! {
            result = wait_for_shutdown_signal() => {
                if let Err(e) = result {
                    error!("Error waiting for shutdown signal: {}", e);
                }
                (true, "OS signal (SIGTERM/SIGINT/Ctrl+C)".to_string())
            }
            reason = shutdown_rx.recv() => {
                match reason {
                    Ok(r) => {
                        info!("Emergency shutdown requested: {}", r);
                        (false, r)
                    }
                    Err(e) => {
                        error!("Error receiving shutdown signal: {}", e);
                        (false, "Emergency shutdown (channel error)".to_string())
                    }
                }
            }
        };

        info!("Shutdown triggered by: {}", shutdown_reason);
        info!("Cleaning up...");

        // Clear voice connections (they will be cleaned up automatically)
        let voice_connections = data.voice_connections.read().await;
        info!(
            "Cleaning up {} voice connection(s)",
            voice_connections.len()
        );

        info!("Cleanup completed");
        let exit_code = if clean_shutdown { 0 } else { 1 };
        std::process::exit(exit_code);
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
