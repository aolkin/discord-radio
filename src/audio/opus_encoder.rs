use opus::{Application, Bitrate, Channels, Encoder};
use std::sync::Arc;

const SAMPLE_RATE: u32 = 48000;
const FRAME_SIZE: usize = 960; // 20ms at 48kHz
const BITRATE: Bitrate = Bitrate::Bits(64000); // 64 kbps for good quality voice

/// Opus encoder wrapper for converting PCM frames to Opus packets
pub struct OpusEncoder {
    encoder: Arc<parking_lot::Mutex<Encoder>>,
}

impl OpusEncoder {
    /// Create a new Opus encoder configured for stereo audio at 48kHz
    pub fn new() -> Result<Self, opus::Error> {
        let mut encoder = Encoder::new(SAMPLE_RATE, Channels::Stereo, Application::Audio)?;
        encoder.set_bitrate(BITRATE)?;

        Ok(Self {
            encoder: Arc::new(parking_lot::Mutex::new(encoder)),
        })
    }

    /// Encode a chunk of PCM frames to Opus
    /// Takes stereo f32 samples and returns an Opus packet
    pub fn encode(&self, frames: &[[f32; 2]]) -> Result<Vec<u8>, opus::Error> {
        if frames.len() != FRAME_SIZE {
            tracing::warn!(
                "Opus encoder expected {} frames but got {}",
                FRAME_SIZE,
                frames.len()
            );
        }

        // Convert [[f32; 2]] to interleaved Vec<f32> for opus encoding
        let mut interleaved = Vec::with_capacity(frames.len() * 2);
        for frame in frames {
            interleaved.push(frame[0]);
            interleaved.push(frame[1]);
        }

        // Encode to opus packet (max size ~4000 bytes is plenty for our use)
        let mut output = vec![0u8; 4000];
        let mut encoder = self.encoder.lock();
        let len = encoder.encode_float(&interleaved, &mut output)?;

        output.truncate(len);
        Ok(output)
    }

    /// Clone the encoder for use in multiple contexts
    #[allow(dead_code)]
    pub fn clone_encoder(&self) -> Self {
        Self {
            encoder: self.encoder.clone(),
        }
    }
}

impl Default for OpusEncoder {
    fn default() -> Self {
        Self::new().expect("Failed to create Opus encoder")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_opus_encoder_creation() {
        let encoder = OpusEncoder::new();
        assert!(encoder.is_ok());
    }

    #[test]
    fn test_opus_encode() {
        let encoder = OpusEncoder::new().unwrap();

        // Create test frames (silence)
        let frames = vec![[0.0f32, 0.0f32]; FRAME_SIZE];

        let result = encoder.encode(&frames);
        assert!(result.is_ok());

        let packet = result.unwrap();
        assert!(!packet.is_empty());
        assert!(packet.len() < 4000); // Should be much smaller than max
    }

    #[test]
    fn test_opus_encode_tone() {
        let encoder = OpusEncoder::new().unwrap();

        // Create test frames (simple sine wave)
        let mut frames = Vec::with_capacity(FRAME_SIZE);
        for i in 0..FRAME_SIZE {
            let t = i as f32 / SAMPLE_RATE as f32;
            let sample = (2.0 * std::f32::consts::PI * 440.0 * t).sin() * 0.5;
            frames.push([sample, sample]);
        }

        let result = encoder.encode(&frames);
        assert!(result.is_ok());

        let packet = result.unwrap();
        assert!(!packet.is_empty());
    }
}
