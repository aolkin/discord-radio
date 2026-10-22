use crate::audio::codec::decoder::OpenedAudio;
use anyhow::{Context, bail};
use rubato::audioadapter_buffers::direct::{InterleavedSlice, SequentialSliceOfVecs};
use rubato::{Fft, FixedSync, Indexing, Resampler};
use std::num::{NonZeroU8, NonZeroU32};
use vorbis_rs::{VorbisBitrateManagementStrategy, VorbisEncoder, VorbisEncoderBuilder};

const CHUNK_FRAMES: usize = 1024;
const VORBIS_QUALITY: f32 = 0.6;

/// Checks that `bytes` decode as `extension`-format audio. Returns `None` if
/// the audio is already at `sample_rate`, or else the audio resampled to
/// `sample_rate` and encoded as Ogg Vorbis.
pub fn convert_upload(
    bytes: Vec<u8>,
    extension: &str,
    sample_rate: u32,
) -> anyhow::Result<Option<Vec<u8>>> {
    let mut audio = OpenedAudio::open(Box::new(std::io::Cursor::new(bytes)), Some(extension))?;
    if audio.sample_rate()? == sample_rate {
        audio.decode_first_packet()?;
        return Ok(None);
    }
    resample_to_ogg(audio, sample_rate).map(Some)
}

fn resample_to_ogg(mut audio: OpenedAudio, sample_rate: u32) -> anyhow::Result<Vec<u8>> {
    let source_rate = audio.sample_rate()?;
    let channels = audio
        .channels()
        .context("Audio file has no channel layout")?;
    if !(1..=2).contains(&channels) {
        bail!("Audio file has {channels} channels; only mono and stereo are supported");
    }
    let mut decoder = audio.make_decoder()?;
    let mut writer = ResampledWriter::new(source_rate, sample_rate, channels)?;

    let mut decoded = Vec::new();
    let mut pending = Vec::new();
    while let Some(packet) = audio.format.next_packet().context("Reading audio")? {
        if packet.track_id != audio.track.id {
            continue;
        }
        decoder
            .decode(&packet)
            .context("Decoding audio")?
            .copy_to_vec_interleaved(&mut decoded);
        pending.extend_from_slice(&decoded);
        let chunk_len = writer.resampler.input_frames_next() * channels;
        let full_len = pending.len() - pending.len() % chunk_len;
        for chunk in pending[..full_len].chunks_exact(chunk_len) {
            writer.write(chunk)?;
        }
        pending.drain(..full_len);
    }
    writer.finish(&pending)
}

struct ResampledWriter {
    resampler: Fft<f32>,
    encoder: VorbisEncoder<Vec<u8>>,
    channels: usize,
    output: Vec<Vec<f32>>,
    delay_frames_left: usize,
    frames_in: usize,
    frames_out: usize,
}

impl ResampledWriter {
    fn new(source_rate: u32, sample_rate: u32, channels: usize) -> anyhow::Result<Self> {
        let resampler = Fft::<f32>::new(
            source_rate as usize,
            sample_rate as usize,
            CHUNK_FRAMES,
            channels,
            FixedSync::Both,
        )
        .context("Creating the resampler")?;
        let encoder = VorbisEncoderBuilder::new_with_serial(
            NonZeroU32::new(sample_rate).context("Sample rate is zero")?,
            NonZeroU8::new(channels as u8).context("Channel count is zero")?,
            Vec::new(),
            rand::random(),
        )
        .bitrate_management_strategy(VorbisBitrateManagementStrategy::QualityVbr {
            target_quality: VORBIS_QUALITY,
        })
        .build()
        .context("Creating the Vorbis encoder")?;
        Ok(Self {
            output: vec![vec![0.0; resampler.output_frames_max()]; channels],
            delay_frames_left: resampler.output_delay(),
            resampler,
            encoder,
            channels,
            frames_in: 0,
            frames_out: 0,
        })
    }

    /// Resamples and encodes interleaved `input`, which holds at most one
    /// chunk of frames, keeping at most `max_frames_out` output frames.
    fn write_limited(&mut self, input: &[f32], max_frames_out: usize) -> anyhow::Result<()> {
        let frames = input.len() / self.channels;
        let indexing = Indexing::new().partial_len(frames);
        let input = InterleavedSlice::new(input, self.channels, frames)?;
        let output_frames = self.output[0].len();
        let mut output =
            SequentialSliceOfVecs::new_mut(&mut self.output, self.channels, output_frames)?;
        let (_, produced) = self
            .resampler
            .process_into_buffer(&input, &mut output, Some(&indexing))
            .context("Resampling audio")?;
        self.frames_in += frames;

        let skip = produced.min(self.delay_frames_left);
        self.delay_frames_left -= skip;
        let keep = (produced - skip).min(max_frames_out);
        if keep > 0 {
            let block: Vec<&[f32]> = self
                .output
                .iter()
                .map(|ch| &ch[skip..skip + keep])
                .collect();
            self.encoder
                .encode_audio_block(block)
                .context("Encoding audio")?;
            self.frames_out += keep;
        }
        Ok(())
    }

    fn write(&mut self, input: &[f32]) -> anyhow::Result<()> {
        self.write_limited(input, usize::MAX)
    }

    fn finish(mut self, remaining: &[f32]) -> anyhow::Result<Vec<u8>> {
        self.write(remaining)?;
        let ratio = self.resampler.resample_ratio();
        let expected = (self.frames_in as f64 * ratio).ceil() as usize;
        if expected == 0 {
            bail!("Audio file has no audio");
        }
        while self.frames_out < expected {
            self.write_limited(&[], expected - self.frames_out)?;
        }
        self.encoder.finish().context("Finishing the Ogg stream")
    }
}
