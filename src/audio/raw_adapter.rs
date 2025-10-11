use crate::audio::processing_thread::AudioProcessor;
use crate::audio::profiles::SignalProfile;
use songbird::input::RawAdapter;
use std::io::{Read, Seek, SeekFrom};
use std::sync::{Arc, Mutex};
use symphonia::core::io::MediaSource;
use tokio::sync::RwLock;

const SAMPLE_RATE: u32 = 48000;
const CHANNELS: u16 = 2;

pub struct ProcessedAudioAdapter {
    processor: Arc<RwLock<AudioProcessor>>,
}

impl ProcessedAudioAdapter {
    pub fn new(initial_profile: SignalProfile) -> Self {
        let processor = Arc::new(RwLock::new(AudioProcessor::new(initial_profile)));

        Self { processor }
    }

    pub async fn start(
        self,
        call: Arc<tokio::sync::Mutex<songbird::Call>>,
    ) -> Arc<RwLock<AudioProcessor>> {
        let processor = self.processor.clone();

        // Create audio reader that generates on-demand
        // Use a std::sync::Mutex wrapper for the processor to allow sync access from Read::read()
        let sync_processor = Arc::new(Mutex::new(processor.clone()));
        let reader = AudioChannelReader::new(sync_processor);

        // Create RawAdapter for Songbird (expects interleaved f32 PCM)
        let raw_input = RawAdapter::new(reader, SAMPLE_RATE, CHANNELS as u32);

        // Play the input through Songbird
        {
            let mut call_guard = call.lock().await;
            call_guard.play_input(raw_input.into());
        }

        processor
    }
}

/// Reader that generates PCM data on-demand when Songbird requests it
struct AudioChannelReader {
    processor: Arc<Mutex<Arc<RwLock<AudioProcessor>>>>,
    buffer: Vec<u8>,
    position: usize,
}

impl AudioChannelReader {
    fn new(processor: Arc<Mutex<Arc<RwLock<AudioProcessor>>>>) -> Self {
        Self {
            processor,
            buffer: Vec::new(),
            position: 0,
        }
    }

    fn generate_audio_chunk(&mut self) {
        // Generate audio on-demand, synchronized with Songbird's playback
        // This is called from a sync context (Songbird's audio thread)
        let processor_guard = self.processor.lock().unwrap();
        let processor_arc = processor_guard.clone();
        drop(processor_guard);

        // Use try_lock to avoid deadlock, fall back to silence if locked
        let frames = if let Ok(mut processor) = processor_arc.try_write() {
            processor.process_next_chunk()
        } else {
            // Processor is locked, return silence for this chunk
            vec![[0.0, 0.0]; 960]
        };

        // Convert frames to interleaved f32 samples, then to bytes
        self.buffer.clear();
        for frame in frames {
            self.buffer.extend_from_slice(&frame[0].to_le_bytes());
            self.buffer.extend_from_slice(&frame[1].to_le_bytes());
        }
        self.position = 0;
    }
}

impl Read for AudioChannelReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        // If we've consumed the current buffer, generate more audio
        if self.position >= self.buffer.len() {
            self.generate_audio_chunk();
        }

        // Copy available data to output buffer
        let available = self.buffer.len() - self.position;
        let to_copy = available.min(buf.len());

        buf[..to_copy].copy_from_slice(&self.buffer[self.position..self.position + to_copy]);
        self.position += to_copy;

        Ok(to_copy)
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
