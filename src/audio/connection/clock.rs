use crate::audio::processing::processor::ProcessorRef;
use serenity::model::id::GuildId;
use std::time::Duration;
use tokio::task::JoinHandle;
use tokio::time::Instant;
use tracing::Instrument;

const CHUNK_DURATION: Duration = Duration::from_millis(20);
const MAX_LAG: Duration = Duration::from_millis(200);

pub struct AudioClock {
    task: JoinHandle<()>,
}

impl AudioClock {
    pub fn start(guild_id: GuildId, processor: ProcessorRef) -> Self {
        tracing::info!("Starting the audio clock");
        let task = tokio::spawn(
            async move {
                let mut deadline = Instant::now();
                loop {
                    tokio::time::sleep_until(deadline).await;
                    processor.write().await.process_next_chunk();
                    deadline = next_deadline(deadline, Instant::now());
                }
            }
            .instrument(tracing::info_span!("audio_clock", guild_id = %guild_id)),
        );
        Self { task }
    }

    pub async fn stop(mut self) {
        self.task.abort();
        let _ = (&mut self.task).await;
        tracing::info!("Stopped the audio clock");
    }
}

impl Drop for AudioClock {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn next_deadline(deadline: Instant, now: Instant) -> Instant {
    let next = deadline + CHUNK_DURATION;
    let lag = now.saturating_duration_since(next);
    if lag > MAX_LAG {
        tracing::warn!(
            "Audio clock fell {:.1}ms behind, skipping ahead",
            lag.as_secs_f64() * 1000.0
        );
        return now;
    }
    next
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_deadline_keeps_the_schedule_and_skips_long_stalls() {
        let start = Instant::now();
        assert_eq!(next_deadline(start, start), start + CHUNK_DURATION);
        let behind = start + CHUNK_DURATION + MAX_LAG;
        assert_eq!(next_deadline(start, behind), start + CHUNK_DURATION);
        let stalled = behind + Duration::from_millis(1);
        assert_eq!(next_deadline(start, stalled), stalled);
    }
}
