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
