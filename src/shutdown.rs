use crate::state::Data;
use anyhow::Context;
use tokio::signal;

pub async fn setup_shutdown_handler(data: Data) {
    tokio::spawn(async move {
        if let Err(e) = wait_for_shutdown_signal().await {
            tracing::error!("Error waiting for shutdown signal: {e:#}");
        }

        tracing::info!("Shutdown triggered by OS signal (SIGTERM/SIGINT/Ctrl+C)");
        tracing::info!("Cleaning up...");

        // Clear voice connections (they will be cleaned up automatically)
        tracing::info!(
            "Cleaning up {} voice connection(s)",
            data.voice_connection_count().await
        );

        tracing::info!("Cleanup completed");
        std::process::exit(0);
    });
}

async fn wait_for_shutdown_signal() -> anyhow::Result<()> {
    #[cfg(unix)]
    {
        use signal::unix::{SignalKind, signal};

        let mut sigterm = signal(SignalKind::terminate()).context("registering SIGTERM handler")?;
        let mut sigint = signal(SignalKind::interrupt()).context("registering SIGINT handler")?;

        tokio::select! {
            _ = sigterm.recv() => {
                tracing::info!("Received SIGTERM");
            }
            _ = sigint.recv() => {
                tracing::info!("Received SIGINT");
            }
        }
    }

    #[cfg(windows)]
    {
        signal::ctrl_c().await?;
        tracing::info!("Received Ctrl+C");
    }

    Ok(())
}
