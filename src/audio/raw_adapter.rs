use crate::audio::processing_thread::{AudioProcessor, ProcessingThread};
use crate::audio::profiles::SignalProfile;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};

const CHANNEL_BUFFER_SIZE: usize = 16;

pub struct ProcessedAudioAdapter {
    processing_thread: ProcessingThread,
    processor_handle: Arc<RwLock<AudioProcessor>>,
}

impl ProcessedAudioAdapter {
    pub fn new(initial_profile: SignalProfile) -> Self {
        let processing_thread = ProcessingThread::new(initial_profile);
        let processor_handle = processing_thread.processor();

        Self {
            processing_thread,
            processor_handle,
        }
    }

    pub fn processor(&self) -> Arc<RwLock<AudioProcessor>> {
        Arc::clone(&self.processor_handle)
    }

    pub async fn spawn(
        self,
    ) -> (
        Arc<RwLock<AudioProcessor>>,
        tokio::task::JoinHandle<Result<(), Box<dyn std::error::Error + Send + Sync>>>,
    ) {
        let processor = self.processor_handle.clone();
        let (tx, _rx) = mpsc::channel::<Vec<i16>>(CHANNEL_BUFFER_SIZE);

        let thread_handle = self.processing_thread.spawn(tx).await;

        (processor, thread_handle)
    }
}
