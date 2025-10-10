use songbird::tracks::TrackHandle;
use tokio::time::{Duration, sleep};
use tokio_util::sync::CancellationToken;

pub async fn fade_volume(
    handle: TrackHandle,
    from_volume: f32,
    to_volume: f32,
    duration_secs: f32,
    cancel_token: CancellationToken,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if duration_secs <= 0.0 {
        handle.set_volume(to_volume)?;
        return Ok(());
    }

    const UPDATE_INTERVAL_MS: u64 = 50;
    let total_steps = (duration_secs * 1000.0 / UPDATE_INTERVAL_MS as f32) as i32;
    let volume_step = (to_volume - from_volume) / total_steps as f32;

    for step in 0..total_steps {
        if cancel_token.is_cancelled() {
            return Ok(());
        }

        let current_volume = from_volume + (volume_step * step as f32);
        handle.set_volume(current_volume)?;

        tokio::select! {
            _ = sleep(Duration::from_millis(UPDATE_INTERVAL_MS)) => {}
            _ = cancel_token.cancelled() => {
                return Ok(());
            }
        }
    }

    handle.set_volume(to_volume)?;

    Ok(())
}
