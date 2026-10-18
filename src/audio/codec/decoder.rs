use crate::audio::processing::mixer::AudioSource;
use anyhow::{Context, bail};
use std::path::Path;
use std::time::Duration;
use symphonia::core::codecs::CodecParameters;
use symphonia::core::codecs::audio::{AudioCodecParameters, AudioDecoder, AudioDecoderOptions};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo, Track, TrackType};
use symphonia::core::io::{MediaSource, MediaSourceStream};
use symphonia::core::meta::MetadataOptions;
use symphonia::core::units::Time;

/// A probed audio container with its default audio track selected.
pub struct OpenedAudio {
    pub format: Box<dyn FormatReader>,
    pub track: Track,
    params: AudioCodecParameters,
}

impl OpenedAudio {
    /// Probes `source` for a container format, using `extension` as a hint.
    pub fn open(source: Box<dyn MediaSource>, extension: Option<&str>) -> anyhow::Result<Self> {
        let mss = MediaSourceStream::new(source, Default::default());
        let mut hint = Hint::new();
        if let Some(extension) = extension {
            hint.with_extension(extension);
        }

        let format = symphonia::default::get_probe()
            .probe(
                &hint,
                mss,
                FormatOptions::default(),
                MetadataOptions::default(),
            )
            .context("Unrecognized audio format")?;
        let track = format
            .default_track(TrackType::Audio)
            .context("No default audio track found")?
            .clone();
        let Some(CodecParameters::Audio(params)) = track.codec_params.clone() else {
            bail!("No default audio track found");
        };

        Ok(Self {
            format,
            track,
            params,
        })
    }

    /// Fails unless the track's sample rate is `required_sample_rate`.
    pub fn require_sample_rate(&self, required_sample_rate: u32) -> anyhow::Result<u32> {
        let sample_rate = self
            .params
            .sample_rate
            .context("Audio file has no sample rate")?;
        if sample_rate != required_sample_rate {
            bail!(
                "Audio file has sample rate {sample_rate}Hz, but {required_sample_rate}Hz is required. Please resample the file first."
            );
        }
        Ok(sample_rate)
    }

    fn make_decoder(&self) -> anyhow::Result<Box<dyn AudioDecoder>> {
        symphonia::default::get_codecs()
            .make_audio_decoder(&self.params, &AudioDecoderOptions::default())
            .context("Unsupported audio codec")
    }
}

/// Fails unless `bytes` decode as `extension`-format audio at
/// `required_sample_rate`. Decodes the first packet, so a file that probes
/// but cannot be decoded fails too.
pub fn check_sample_rate(
    bytes: Vec<u8>,
    extension: &str,
    required_sample_rate: u32,
) -> anyhow::Result<()> {
    let mut audio = OpenedAudio::open(Box::new(std::io::Cursor::new(bytes)), Some(extension))?;
    audio.require_sample_rate(required_sample_rate)?;

    let mut decoder = audio.make_decoder()?;
    loop {
        let packet = audio
            .format
            .next_packet()
            .context("Reading the first packet")?
            .context("Audio file has no packets")?;
        if packet.track_id == audio.track.id {
            decoder
                .decode(&packet)
                .context("Decoding the first packet")?;
            return Ok(());
        }
    }
}

pub struct SymphoniaSource {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn AudioDecoder>,
    track_id: u32,
    sample_buf: Vec<f32>,
    current_frame_idx: usize,
    sample_rate: u32,
    channels: usize,
    file_path: String,
}

impl SymphoniaSource {
    pub fn from_file(path: impl AsRef<Path>, required_sample_rate: u32) -> anyhow::Result<Self> {
        let path_str = path.as_ref().to_string_lossy().to_string();
        let file = std::fs::File::open(&path).with_context(|| format!("opening {path_str}"))?;
        let extension = path.as_ref().extension().and_then(|e| e.to_str());

        let audio = OpenedAudio::open(Box::new(file), extension)?;
        let sample_rate = audio.require_sample_rate(required_sample_rate)?;
        let decoder = audio.make_decoder()?;
        let track_id = audio.track.id;
        let format = audio.format;

        let channels = audio
            .params
            .channels
            .as_ref()
            .map(|ch| ch.count())
            .unwrap_or(2);

        tracing::debug!(
            "Loaded audio file: {} (sample rate: {}Hz, channels: {})",
            path_str,
            sample_rate,
            channels
        );

        Ok(Self {
            format,
            decoder,
            track_id,
            sample_buf: Vec::new(),
            current_frame_idx: 0,
            sample_rate,
            channels,
            file_path: path_str,
        })
    }

    /// Decodes the next packet of the track into `sample_buf`. Returns
    /// `false` at the end of the stream.
    fn decode_next_packet(&mut self) -> Result<bool, SymphoniaError> {
        loop {
            let Some(packet) = self.format.next_packet()? else {
                return Ok(false);
            };

            if packet.track_id != self.track_id {
                continue;
            }

            let audio_buf = self.decoder.decode(&packet)?;
            // Skip empty buffers (can happen after seeking)
            if audio_buf.frames() == 0 {
                continue;
            }

            audio_buf.copy_to_vec_interleaved(&mut self.sample_buf);
            self.current_frame_idx = 0;
            return Ok(true);
        }
    }

    fn get_next_source_frame(&mut self) -> Option<[f32; 2]> {
        // Need to decode if we don't have a buffer or have consumed current buffer
        let needs_decode = self.current_frame_idx >= self.sample_buf.len() / self.channels;

        if needs_decode && !self.decode_next_packet().unwrap_or(false) {
            return None;
        }

        let channels = self.channels;
        let samples = &self.sample_buf;
        let num_frames = samples.len() / channels;

        if self.current_frame_idx >= num_frames {
            tracing::warn!(
                "Frame index {} >= num_frames {} (buf.len={}, channels={})",
                self.current_frame_idx,
                num_frames,
                samples.len(),
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

    fn seek(&mut self, position: Duration) -> anyhow::Result<()> {
        let time = Time::try_from_secs_f64(position.as_secs_f64())
            .context("Seek position out of range")?;

        self.format.seek(
            SeekMode::Accurate,
            SeekTo::Time {
                time,
                track_id: None,
            },
        )?;

        // Reset decoder to clear any buffered state
        self.decoder.reset();

        // Reset buffer state after seek
        self.current_frame_idx = 0;
        self.sample_buf.clear();

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
                SeekMode::Accurate,
                SeekTo::Time {
                    time: Time::ZERO,
                    track_id: None,
                },
            )
            .is_ok()
        {
            self.current_frame_idx = 0;
            self.sample_buf.clear();
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

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48000;

    /// 16-bit stereo PCM WAV holding `frames` frames of a ramp.
    fn wav_bytes(frames: u32) -> Vec<u8> {
        let data_len = frames * 4;
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + data_len).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&RATE.to_le_bytes());
        wav.extend_from_slice(&(RATE * 4).to_le_bytes());
        wav.extend_from_slice(&4u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&data_len.to_le_bytes());
        for i in 0..frames {
            let sample = (i as i16).wrapping_mul(10);
            wav.extend_from_slice(&sample.to_le_bytes());
            wav.extend_from_slice(&sample.to_le_bytes());
        }
        wav
    }

    #[test]
    fn decodes_seeks_and_resets_wav() {
        let frames = 4800;
        check_sample_rate(wav_bytes(frames), "wav", RATE).unwrap();
        assert!(check_sample_rate(wav_bytes(frames), "wav", 44100).is_err());

        let path = std::env::temp_dir().join(format!("decoder_test_{}.wav", std::process::id()));
        std::fs::write(&path, wav_bytes(frames)).unwrap();
        let mut source = SymphoniaSource::from_file(&path, RATE).unwrap();

        let mut count = 0;
        while source.next_frame().is_some() {
            count += 1;
        }
        assert_eq!(count, frames);

        source.reset();
        assert_eq!(source.next_frame(), Some([0.0, 0.0]));

        source.seek(Duration::from_millis(50)).unwrap();
        let mut remaining = 0;
        while source.next_frame().is_some() {
            remaining += 1;
        }
        assert!((frames - 2400..frames).contains(&remaining));

        std::fs::remove_file(path).unwrap();
    }
}
