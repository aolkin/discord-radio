use crate::bucket::registry::{JsonRegistry, RegistryEntry};
use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct SignalParameters {
    pub name: String,

    pub bandpass_low: f32,
    pub bandpass_high: f32,

    /// Noise levels (0.0-2.0) use perceptual (dB-based) scaling:
    /// - 0.0 = silence
    /// - 0.5 = moderate (-30dB)
    /// - 1.0 = reference level (0dB)
    /// - 2.0 = boosted (+18dB)
    pub white_noise_level: f32,
    pub pink_noise_level: f32,
    pub brown_noise_level: f32,

    pub tremolo_depth: f32,
    pub tremolo_rate: f32,
    pub tremolo_jitter: f32,

    pub clip_pregain: f32,
    pub clip_threshold: f32,
    pub bitcrush_bits: Option<u8>,

    pub dropout_probability: f32,
    pub dropout_duration_ms: (f32, f32),
}

impl SignalParameters {
    pub fn interpolate(&self, other: &Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        let inv_t = 1.0 - t;

        Self {
            name: format!("{}→{}", self.name, other.name),
            bandpass_low: self.bandpass_low * inv_t + other.bandpass_low * t,
            bandpass_high: self.bandpass_high * inv_t + other.bandpass_high * t,
            // Interpolate noise levels linearly in perceptual space.
            // The perceptual scale (0.0-2.0) already maps linearly to dB,
            // so linear interpolation here gives smooth perceptual transitions.
            white_noise_level: self.white_noise_level * inv_t + other.white_noise_level * t,
            pink_noise_level: self.pink_noise_level * inv_t + other.pink_noise_level * t,
            brown_noise_level: self.brown_noise_level * inv_t + other.brown_noise_level * t,
            tremolo_depth: self.tremolo_depth * inv_t + other.tremolo_depth * t,
            tremolo_rate: self.tremolo_rate * inv_t + other.tremolo_rate * t,
            tremolo_jitter: self.tremolo_jitter * inv_t + other.tremolo_jitter * t,
            clip_pregain: self.clip_pregain * inv_t + other.clip_pregain * t,
            clip_threshold: self.clip_threshold * inv_t + other.clip_threshold * t,
            bitcrush_bits: if t < 0.5 {
                self.bitcrush_bits
            } else {
                other.bitcrush_bits
            },
            dropout_probability: self.dropout_probability * inv_t + other.dropout_probability * t,
            dropout_duration_ms: (
                self.dropout_duration_ms.0 * inv_t + other.dropout_duration_ms.0 * t,
                self.dropout_duration_ms.1 * inv_t + other.dropout_duration_ms.1 * t,
            ),
        }
    }
}

impl SignalParameters {
    pub fn patched(&self, patch: &Map<String, Value>) -> anyhow::Result<Self> {
        let Value::Object(mut fields) = serde_json::to_value(self)? else {
            bail!("signal parameters are not a JSON object");
        };
        for (field, value) in patch {
            if field == "name" || !fields.contains_key(field) {
                bail!("{field:?} is not an editable signal parameter");
            }
            fields.insert(field.clone(), value.clone());
        }
        let patched: Self = serde_json::from_value(Value::Object(fields))
            .context("reading the edited signal parameters")?;
        patched.check_limits()?;
        Ok(patched)
    }
}

const BANDPASS_HZ: (f32, f32) = (20.0, 20000.0);
const NOISE_LEVEL: (f32, f32) = (0.0, 2.0);
const UNIT: (f32, f32) = (0.0, 1.0);
const TREMOLO_RATE_HZ: (f32, f32) = (0.0, 20.0);
const CLIP_PREGAIN: (f32, f32) = (0.0, 100.0);
const CLIP_THRESHOLD: (f32, f32) = (0.0, 2.0);
const BITCRUSH_BITS: (u8, u8) = (1, 16);
const DROPOUT_MS: (f32, f32) = (0.0, 2000.0);

fn check(field: &str, value: f32, (min, max): (f32, f32)) -> anyhow::Result<()> {
    if !(min..=max).contains(&value) {
        bail!("{field} must be between {min} and {max}, got {value}");
    }
    Ok(())
}

impl SignalParameters {
    fn check_limits(&self) -> anyhow::Result<()> {
        check("bandpass_low", self.bandpass_low, BANDPASS_HZ)?;
        check("bandpass_high", self.bandpass_high, BANDPASS_HZ)?;
        if self.bandpass_low >= self.bandpass_high {
            bail!("bandpass_low must be below bandpass_high");
        }
        check("white_noise_level", self.white_noise_level, NOISE_LEVEL)?;
        check("pink_noise_level", self.pink_noise_level, NOISE_LEVEL)?;
        check("brown_noise_level", self.brown_noise_level, NOISE_LEVEL)?;
        check("tremolo_depth", self.tremolo_depth, UNIT)?;
        check("tremolo_rate", self.tremolo_rate, TREMOLO_RATE_HZ)?;
        check("tremolo_jitter", self.tremolo_jitter, UNIT)?;
        check("clip_pregain", self.clip_pregain, CLIP_PREGAIN)?;
        check("clip_threshold", self.clip_threshold, CLIP_THRESHOLD)?;
        if let Some(bits) = self.bitcrush_bits {
            let (min, max) = BITCRUSH_BITS;
            if !(min..=max).contains(&bits) {
                bail!("bitcrush_bits must be between {min} and {max}, got {bits}");
            }
        }
        check("dropout_probability", self.dropout_probability, UNIT)?;
        let (shortest, longest) = self.dropout_duration_ms;
        check("dropout_duration_ms min", shortest, DROPOUT_MS)?;
        check("dropout_duration_ms max", longest, DROPOUT_MS)?;
        if shortest > longest {
            bail!("dropout_duration_ms min must not exceed max");
        }
        Ok(())
    }
}

impl RegistryEntry for SignalParameters {
    const KIND: &'static str = "signal preset";
    const DEFAULTS: &'static [&'static str] = &[
        include_str!("default_presets/clear_signal.json"),
        include_str!("default_presets/strong_signal.json"),
        include_str!("default_presets/fading_signal.json"),
        include_str!("default_presets/weak_signal.json"),
        include_str!("default_presets/noise_pink.json"),
        include_str!("default_presets/noise_brown.json"),
        include_str!("default_presets/noise_white.json"),
    ];

    fn name(&self) -> &str {
        &self.name
    }

    fn validate(&self) -> anyhow::Result<()> {
        self.check_limits()
    }
}

impl JsonRegistry<SignalParameters> {
    pub fn default_preset(&self) -> SignalParameters {
        (*self
            .get("clear_signal")
            .expect("clear_signal is a default preset"))
        .clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clear() -> SignalParameters {
        serde_json::from_str(include_str!("default_presets/clear_signal.json")).unwrap()
    }

    #[test]
    fn validate_rejects_values_that_break_the_dsp() {
        assert!(clear().validate().is_ok());
        let cases: [fn(&mut SignalParameters); 5] = [
            |p| p.bandpass_high = 24000.0,
            |p| p.bandpass_low = 20000.0,
            |p| p.bandpass_low = 0.0,
            |p| p.dropout_duration_ms = (100.0, 50.0),
            |p| p.bitcrush_bits = Some(255),
        ];
        for mutate in cases {
            let mut preset = clear();
            mutate(&mut preset);
            assert!(preset.validate().is_err());
        }
        let patch = |value: Value| clear().patched(value.as_object().unwrap());
        assert_eq!(
            patch(serde_json::json!({ "tremolo_depth": 0.5 }))
                .unwrap()
                .tremolo_depth,
            0.5
        );
        assert!(patch(serde_json::json!({ "bandpass_high": 24000.0 })).is_err());
        assert!(patch(serde_json::json!({ "name": "other" })).is_err());
        assert!(patch(serde_json::json!({ "volume": 1.0 })).is_err());
    }
}
