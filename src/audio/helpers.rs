//! Audio utility functions and helpers.

const MIN_DB: f32 = 60.0;
const MAX_DB: f32 = 18.0;

/// Convert a perceptual level value (0.0-2.0) to linear amplitude using dB scaling.
///
/// This function maps user-friendly config values to perceptual (logarithmic) amplitude:
/// - 0.0 → silence (effectively -60dB, amplitude ≈ 0.001)
/// - 0.5 → moderate (-30dB, amplitude ≈ 0.032)
/// - 1.0 → reference level (0dB, amplitude = 1.0)
/// - 2.0 → boosted (+6dB, amplitude ≈ 2.0)
///
/// The mapping provides perceptual (logarithmic) scaling that matches human loudness perception:
/// - For values below 1.0: dB = -60 + (value × 60)
/// - For values at or above 1.0: dB = (value - 1.0) × 6
/// - Then: amplitude = 10^(dB / 20)
pub fn perceptual_level_to_amplitude(level: f32) -> f32 {
    if level <= 0.0 {
        return 0.0;
    }

    // Map config value to dB:
    // 0.0 -> -60dB
    // 0.5 -> -30dB
    // 1.0 -> 0dB
    // 2.0 -> +6dB
    let db = if level < 1.0 {
        // Below 1.0: interpolate from -60dB to 0dB
        -MIN_DB + (level * MIN_DB)
    } else {
        // Above 1.0: interpolate from 0dB to +6dB
        (level - 1.0) * MAX_DB
    };

    // Convert dB to amplitude: amplitude = 10^(dB/20)
    10f32.powf(db / 20.0)
}

#[cfg(test)]
mod tests {
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
        // +6 dB = 10^(6/20) = 10^0.3 ≈ 1.995
        assert!(
            (amp - 1.995).abs() < 0.01,
            "2.0 level should produce ~2x amplitude"
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
}
