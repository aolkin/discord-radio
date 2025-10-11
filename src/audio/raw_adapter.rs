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
    last_generation_time: Option<std::time::Instant>,
    total_frames_generated: u64,
}

impl AudioChannelReader {
    fn new(processor: Arc<Mutex<Arc<RwLock<AudioProcessor>>>>) -> Self {
        Self {
            processor,
            buffer: Vec::new(),
            position: 0,
            last_generation_time: None,
            total_frames_generated: 0,
        }
    }

    fn generate_audio_chunk(&mut self) {
        let now = std::time::Instant::now();

        // Generate a fixed amount of audio each time (5 chunks = 100ms)
        // This matches Songbird's typical consumption pattern
        const CHUNK_FRAMES: usize = 960;
        const CHUNKS_TO_GENERATE: usize = 5; // 100ms worth of audio
        const EXPECTED_INTERVAL_MS: f64 = 20.0 * CHUNKS_TO_GENERATE as f64;

        // Log timing between chunk generations
        if let Some(last_time) = self.last_generation_time {
            let elapsed_ms = now.duration_since(last_time).as_secs_f64() * 1000.0;
            let drift_ms = elapsed_ms - EXPECTED_INTERVAL_MS;

            // Log if drift is significant (more than 10ms off)
            if drift_ms.abs() > 10.0 {
                tracing::warn!(
                    "Audio generation timing drift: {:.2}ms elapsed (expected {:.2}ms), drift: {:.2}ms, total_frames: {}",
                    elapsed_ms,
                    EXPECTED_INTERVAL_MS,
                    drift_ms,
                    self.total_frames_generated
                );
            }
        }
        self.last_generation_time = Some(now);

        // Generate audio on-demand, synchronized with Songbird's playback
        let processor_guard = self.processor.lock().unwrap();
        let processor_arc = processor_guard.clone();
        drop(processor_guard);

        // Generate fixed number of chunks for consistent timing
        self.buffer.clear();

        // Lock once for all chunks to ensure consistency
        let generation_start = std::time::Instant::now();
        match processor_arc.try_write() {
            Ok(mut processor) => {
                // Generate all chunks with the lock held
                for _ in 0..CHUNKS_TO_GENERATE {
                    let frames = processor.process_next_chunk();
                    self.total_frames_generated += frames.len() as u64;

                    // Convert frames to interleaved f32 samples, then to bytes
                    for frame in frames {
                        self.buffer.extend_from_slice(&frame[0].to_le_bytes());
                        self.buffer.extend_from_slice(&frame[1].to_le_bytes());
                    }
                }

                // Check if we took too long to generate audio
                let total_generation_ms = generation_start.elapsed().as_secs_f64() * 1000.0;
                let expected_duration_ms =
                    (CHUNKS_TO_GENERATE * CHUNK_FRAMES) as f64 / 48000.0 * 1000.0;
                if total_generation_ms > expected_duration_ms {
                    tracing::warn!(
                        "Audio generation batch too slow: took {:.2}ms to generate {:.2}ms of audio ({}% real-time)",
                        total_generation_ms,
                        expected_duration_ms,
                        (expected_duration_ms / total_generation_ms * 100.0) as u32
                    );
                }
            }
            Err(_) => {
                // Processor is locked, return silence for entire batch
                tracing::warn!(
                    "Audio processor locked during generation, returning silence for {}ms",
                    CHUNKS_TO_GENERATE * 20
                );
                let silence_frames = CHUNK_FRAMES * CHUNKS_TO_GENERATE;
                for _ in 0..silence_frames {
                    self.buffer.extend_from_slice(&0.0f32.to_le_bytes());
                    self.buffer.extend_from_slice(&0.0f32.to_le_bytes());
                }
            }
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
