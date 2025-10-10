use crate::audio::custom_mixer::AudioSource;
use rubato::{FastFixedIn, PolynomialDegree};
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
    total_frames: Option<u64>,
    resampler: Option<FastFixedIn<f32>>,
    resample_buffer: Vec<Vec<f32>>,
    source_sample_rate: u32,
    target_sample_rate: u32,
    file_path: String,
}

impl SymphoniaSource {
    pub fn from_file(
        path: impl AsRef<Path>,
        target_sample_rate: u32,
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

        let probed = symphonia::default::get_probe().format(&hint, mss, &format_opts, &metadata_opts)?;

        let format = probed.format;
        let track = format
            .default_track()
            .ok_or("No default audio track found")?;

        let track_id = track.id;
        let decoder_opts = DecoderOptions::default();
        let decoder = symphonia::default::get_codecs().make(&track.codec_params, &decoder_opts)?;

        let source_sample_rate = track.codec_params.sample_rate.unwrap_or(target_sample_rate);

        let total_frames = track.codec_params.n_frames;

        let (resampler, resample_buffer) = if source_sample_rate != target_sample_rate {
            let resampler = FastFixedIn::<f32>::new(
                target_sample_rate as f64 / source_sample_rate as f64,
                1.0,
                PolynomialDegree::Septic,
                1024,
                2,
            )?;

            let resample_buffer = vec![vec![0.0; 1024]; 2];
            (Some(resampler), resample_buffer)
        } else {
            (None, Vec::new())
        };

        Ok(Self {
            format,
            decoder,
            track_id,
            sample_buf: None,
            current_frame_idx: 0,
            total_frames,
            resampler,
            resample_buffer,
            source_sample_rate,
            target_sample_rate,
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
                    if self.sample_buf.is_none() || self.sample_buf.as_ref().unwrap().capacity() < audio_buf.capacity() {
                        self.sample_buf = Some(SampleBuffer::new(audio_buf.capacity() as u64, *audio_buf.spec()));
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
        if (self.sample_buf.is_none() ||
           self.current_frame_idx >= self.sample_buf.as_ref().unwrap().len())
            && self.decode_next_packet().is_err() {
                return None;
            }

        let buf = self.sample_buf.as_ref()?;
        let channels = if !buf.is_empty() { buf.samples().len() / buf.len() } else { 2 };
        let samples = buf.samples();

        if self.current_frame_idx >= buf.len() {
            return None;
        }

        let frame = if channels == 1 {
            let mono = samples[self.current_frame_idx];
            [mono, mono]
        } else {
            let left = samples[self.current_frame_idx * channels];
            let right = samples[self.current_frame_idx * channels + 1];
            [left, right]
        };

        self.current_frame_idx += 1;
        Some(frame)
    }
}

impl AudioSource for SymphoniaSource {
    fn next_frame(&mut self) -> Option<[f32; 2]> {
        if let Some(ref mut _resampler) = self.resampler {
            todo!("Implement resampling")
        } else {
            self.get_next_source_frame()
        }
    }

    fn seek(&mut self, _position: Duration) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Ok(())
    }

    fn duration(&self) -> Option<Duration> {
        self.total_frames.map(|frames| {
            let seconds = frames as f64 / self.source_sample_rate as f64;
            Duration::from_secs_f64(seconds)
        })
    }

    fn reset(&mut self) {
        let _ = Self::from_file(&self.file_path, self.target_sample_rate).map(|new_source| {
            *self = new_source;
        });
    }
}
