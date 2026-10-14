//! Audio utility functions and helpers.

const MIN_DB: f32 = 60.0;
const MAX_DB: f32 = 18.0;

/// Convert a perceptual level value (0.0-2.0) to linear amplitude using dB scaling.
///
/// This function maps user-friendly config values to perceptual (logarithmic) amplitude:
/// - 0.0 → silence (amplitude 0)
/// - 0.5 → moderate (-30dB, amplitude ≈ 0.032)
/// - 1.0 → reference level (0dB, amplitude = 1.0)
/// - 2.0 → boosted (+18dB, amplitude ≈ 7.94)
///
/// The mapping provides perceptual (logarithmic) scaling that matches human loudness perception:
/// - For values below 1.0: dB = -60 + (value × 60)
/// - For values at or above 1.0: dB = (value - 1.0) × 18
/// - Then: amplitude = 10^(dB / 20)
pub fn perceptual_level_to_amplitude(level: f32) -> f32 {
    if level <= 0.0 {
        return 0.0;
    }

    // Map config value to dB:
    // 0.0 -> -60dB
    // 0.5 -> -30dB
    // 1.0 -> 0dB
    // 2.0 -> +18dB
    let db = if level < 1.0 {
        // Below 1.0: interpolate from -60dB to 0dB
        -MIN_DB + (level * MIN_DB)
    } else {
        // Above 1.0: interpolate from 0dB to +18dB
        (level - 1.0) * MAX_DB
    };

    // Convert dB to amplitude: amplitude = 10^(dB/20)
    10f32.powf(db / 20.0)
}

#[cfg(test)]
mod tests;
