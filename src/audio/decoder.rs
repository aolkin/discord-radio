use crate::audio::custom_mixer::AudioSource;
use std::path::Path;
use std::time::Duration;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{Decoder, DecoderOptions};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::{FormatOptions, FormatReader};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

pub struct SymphoniaSource {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn Decoder>,
    track_id: u32,
    sample_buf: Option<SampleBuffer<f32>>,
    current_frame_idx: usize,
    sample_rate: u32,
    channels: usize,
    file_path: String,
}

impl SymphoniaSource {
    pub fn from_file(
        path: impl AsRef<Path>,
        required_sample_rate: u32,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let path_str = path.as_ref().to_string_lossy().to_string();
        let file = std::fs::File::open(&path)?;
        let mss = MediaSourceStream::new(Box::new(file), Default::default());

        let mut hint = Hint::new();
        if let Some(ext) = path.as_ref().extension().and_then(|e| e.to_str()) {
            hint.with_extension(ext);
        }

        let format_opts = FormatOptions::default();
        let metadata_opts = MetadataOptions::default();

        let probed =
            symphonia::default::get_probe().format(&hint, mss, &format_opts, &metadata_opts)?;

        let format = probed.format;
        let track = format
            .default_track()
            .ok_or("No default audio track found")?;

        let track_id = track.id;
        let decoder_opts = DecoderOptions::default();
        let decoder = symphonia::default::get_codecs().make(&track.codec_params, &decoder_opts)?;

        let sample_rate = track
            .codec_params
            .sample_rate
            .ok_or("Audio file has no sample rate")?;

        // Validate sample rate matches required rate
        if sample_rate != required_sample_rate {
            return Err(format!(
                "Audio file has sample rate {}Hz, but {}Hz is required. Please resample the file first.",
                sample_rate,
                required_sample_rate
            ).into());
        }

        let channels = track
            .codec_params
            .channels
            .map(|ch| ch.count())
            .unwrap_or(2);

        tracing::info!(
            "Loaded audio file: {} (sample rate: {}Hz, channels: {})",
            path_str,
            sample_rate,
            channels
        );

        Ok(Self {
            format,
            decoder,
            track_id,
            sample_buf: None,
            current_frame_idx: 0,
            sample_rate,
            channels,
            file_path: path_str,
        })
    }

    fn decode_next_packet(&mut self) -> Result<(), SymphoniaError> {
        loop {
            let packet = self.format.next_packet()?;

            if packet.track_id() != self.track_id {
                continue;
            }

            match self.decoder.decode(&packet) {
                Ok(audio_buf) => {
                    // Skip empty buffers (can happen after seeking)
                    if audio_buf.frames() == 0 {
                        continue;
                    }

                    if self.sample_buf.is_none()
                        || self.sample_buf.as_ref().unwrap().capacity() < audio_buf.capacity()
                    {
                        self.sample_buf = Some(SampleBuffer::new(
                            audio_buf.capacity() as u64,
                            *audio_buf.spec(),
                        ));
                    }

                    if let Some(ref mut buf) = self.sample_buf {
                        buf.copy_interleaved_ref(audio_buf);
                        self.current_frame_idx = 0;
                    }

                    return Ok(());
                }
                Err(e) => return Err(e),
            }
        }
    }

    fn get_next_source_frame(&mut self) -> Option<[f32; 2]> {
        // Need to decode if we don't have a buffer or have consumed current buffer
        let needs_decode = self
            .sample_buf
            .as_ref()
            .map(|buf| {
                let channels = self.channels;
                let num_frames = buf.len() / channels;
                self.current_frame_idx >= num_frames
            })
            .unwrap_or(true);

        if needs_decode && self.decode_next_packet().is_err() {
            return None;
        }

        let buf = self.sample_buf.as_ref()?;
        let channels = self.channels;
        let samples = buf.samples();
        let num_frames = buf.len() / channels;

        if self.current_frame_idx >= num_frames {
            tracing::warn!(
                "Frame index {} >= num_frames {} (buf.len={}, channels={})",
                self.current_frame_idx,
                num_frames,
                buf.len(),
                channels
            );
            return None;
        }

        let frame = if channels == 1 {
            let mono = samples[self.current_frame_idx];
            [mono, mono]
        } else if channels >= 2 {
            let left = samples[self.current_frame_idx * channels];
            let right = samples[self.current_frame_idx * channels + 1];
            [left, right]
        } else {
            tracing::error!("Unexpected channel count: {}", channels);
            return None;
        };

        self.current_frame_idx += 1;
        Some(frame)
    }
}

impl AudioSource for SymphoniaSource {
    fn next_frame(&mut self) -> Option<[f32; 2]> {
        self.get_next_source_frame()
    }

    fn seek(&mut self, position: Duration) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let total_seconds = position.as_secs_f64();
        let seconds = position.as_secs();
        let frac = total_seconds - seconds as f64;

        self.format.seek(
            symphonia::core::formats::SeekMode::Accurate,
            symphonia::core::formats::SeekTo::Time {
                time: symphonia::core::units::Time::new(seconds, frac),
                track_id: None,
            },
        )?;

        // Reset decoder to clear any buffered state
        self.decoder.reset();

        // Reset buffer state after seek
        self.current_frame_idx = 0;
        self.sample_buf = None;

        Ok(())
    }

    fn duration(&self) -> Option<Duration> {
        None // Duration not tracked without total_frames
    }

    fn reset(&mut self) {
        // Try to seek to beginning first
        if self
            .format
            .seek(
                symphonia::core::formats::SeekMode::Accurate,
                symphonia::core::formats::SeekTo::Time {
                    time: symphonia::core::units::Time::new(0, 0.0),
                    track_id: None,
                },
            )
            .is_ok()
        {
            self.current_frame_idx = 0;
            self.sample_buf = None;
            tracing::debug!("Reset audio source by seeking to beginning");
            return;
        }

        // If seek fails, recreate from file
        tracing::debug!("Seek failed, recreating audio source from file");
        match Self::from_file(&self.file_path, self.sample_rate) {
            Ok(new_source) => {
                *self = new_source;
            }
            Err(e) => {
                tracing::error!(
                    "Failed to reset audio source for '{}': {}",
                    self.file_path,
                    e
                );
            }
        }
    }
}
