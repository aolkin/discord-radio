use crate::audio::processing_thread::{AudioProcessor, ProcessingThread};
use crate::audio::profiles::SignalProfile;
use songbird::input::RawAdapter;
use std::io::{Read, Seek, SeekFrom};
use std::sync::Arc;
use symphonia::core::io::MediaSource;
use tokio::sync::{mpsc, RwLock};

const CHANNEL_BUFFER_SIZE: usize = 16;
const SAMPLE_RATE: u32 = 48000;
const CHANNELS: u16 = 2;

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
        call: Arc<tokio::sync::Mutex<songbird::Call>>,
    ) -> (
        Arc<RwLock<AudioProcessor>>,
        tokio::task::JoinHandle<Result<(), Box<dyn std::error::Error + Send + Sync>>>,
    ) {
        let processor = self.processor_handle.clone();
        let (tx, rx) = mpsc::channel::<Vec<i16>>(CHANNEL_BUFFER_SIZE);

        // Create audio reader from the receiver
        let reader = AudioChannelReader::new(rx);

        // Create RawAdapter for Songbird
        let raw_input = RawAdapter::new(reader, SAMPLE_RATE, CHANNELS as u32);

        // Play the input through Songbird
        {
            let mut call_guard = call.lock().await;
            call_guard.play_input(raw_input.into());
        }

        let thread_handle = self.processing_thread.spawn(tx).await;

        (processor, thread_handle)
    }
}

/// Reader that pulls PCM data from a channel
struct AudioChannelReader {
    receiver: mpsc::Receiver<Vec<i16>>,
    buffer: Vec<u8>,
    position: usize,
}

impl AudioChannelReader {
    fn new(receiver: mpsc::Receiver<Vec<i16>>) -> Self {
        Self {
            receiver,
            buffer: Vec::new(),
            position: 0,
        }
    }

    fn fill_buffer_from_channel(&mut self) {
        // Try to receive new audio data (non-blocking)
        if let Ok(pcm_samples) = self.receiver.try_recv() {
            // Convert i16 samples to bytes (little-endian)
            self.buffer.clear();
            for sample in pcm_samples {
                self.buffer.extend_from_slice(&sample.to_le_bytes());
            }
            self.position = 0;
        }
    }
}

impl Read for AudioChannelReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        // If we've consumed the current buffer, try to get more
        if self.position >= self.buffer.len() {
            self.fill_buffer_from_channel();
        }

        // If still no data, return silence
        if self.buffer.is_empty() {
            buf.fill(0);
            return Ok(buf.len());
        }

        // Copy available data to output buffer
        let available = self.buffer.len() - self.position;
        let to_copy = available.min(buf.len());

        buf[..to_copy].copy_from_slice(&self.buffer[self.position..self.position + to_copy]);
        self.position += to_copy;

        // Fill remainder with silence if needed
        if to_copy < buf.len() {
            buf[to_copy..].fill(0);
        }

        Ok(buf.len())
    }
}

impl Seek for AudioChannelReader {
    fn seek(&mut self, _pos: SeekFrom) -> std::io::Result<u64> {
        // Seeking not supported for live stream
        Ok(0)
    }
}

impl MediaSource for AudioChannelReader {
    fn is_seekable(&self) -> bool {
        false
    }

    fn byte_len(&self) -> Option<u64> {
        None // Infinite stream
    }
}
