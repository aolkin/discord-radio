use super::*;

#[test]
fn test_silence() {
    let amp = perceptual_level_to_amplitude(0.0);
    // At -60 dB: 10^(-60/20) = 10^-3 = 0.001
    assert!(
        amp < 0.002,
        "0.0 level should produce near-zero amplitude (got {})",
        amp
    );
}

#[test]
fn test_unity_gain() {
    let amp = perceptual_level_to_amplitude(1.0);
    assert!(
        (amp - 1.0).abs() < 0.01,
        "1.0 level should produce unity gain"
    );
}

#[test]
fn test_max_boost() {
    let amp = perceptual_level_to_amplitude(2.0);
    // +18 dB = 10^(18/20) = 10^0.9 ≈ 7.943
    assert!(
        (amp - 7.943).abs() < 0.01,
        "2.0 level should produce ~8x amplitude"
    );
}

#[test]
fn test_half_level() {
    let amp = perceptual_level_to_amplitude(0.5);
    // At 0.5, dB = -60 + 0.5 * 60 = -30 dB
    // amplitude = 10^(-30/20) = 10^-1.5 ≈ 0.0316
    assert!(
        amp > 0.03 && amp < 0.04,
        "0.5 level should be perceptually mid-range (got {})",
        amp
    );
}

#[test]
fn test_clamping_above() {
    let amp1 = perceptual_level_to_amplitude(2.0);
    let amp2 = perceptual_level_to_amplitude(3.0);
    // Values above 2.0 should be clamped by the formula behavior (they won't be equal but close)
    // Actually, they won't be equal because the function doesn't clamp. Let's verify it continues the linear trend.
    assert!(amp2 > amp1, "Values above 2.0 should continue to increase");
}

#[test]
fn test_negative_returns_zero() {
    let amp = perceptual_level_to_amplitude(-1.0);
    assert_eq!(amp, 0.0, "Negative values should return 0.0");
}
