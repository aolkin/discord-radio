use crate::audio::processing::processor::ProcessorRef;
use serenity::model::id::GuildId;
use std::time::Duration;
use tokio::task::JoinHandle;
use tokio::time::MissedTickBehavior;
use tracing::Instrument;

const CHUNK_DURATION: Duration = Duration::from_millis(20);

pub struct AudioClock {
    guild_id: GuildId,
    task: JoinHandle<()>,
}

impl AudioClock {
    #[tracing::instrument(skip_all, fields(guild_id = %guild_id))]
    pub fn start(guild_id: GuildId, processor: ProcessorRef) -> Self {
        tracing::info!("Starting the audio clock");
        let task = tokio::spawn(
            async move {
                let mut interval = tokio::time::interval(CHUNK_DURATION);
                interval.set_missed_tick_behavior(MissedTickBehavior::Burst);
                loop {
                    interval.tick().await;
                    processor.write().await.process_next_chunk();
                }
            }
            .in_current_span(),
        );
        Self { guild_id, task }
    }

    #[tracing::instrument(skip_all, fields(guild_id = %self.guild_id))]
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
