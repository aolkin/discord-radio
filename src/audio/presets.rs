use crate::bucket::registry::{JsonRegistry, RegistryEntry};
use anyhow::bail;
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct SignalParameters {
    pub name: String,
    pub noise: Vec<Noise>,
    pub bandpass: Bandpass,
    pub tremolo: Tremolo,
    pub clip: Clip,
    pub bitcrush: Bitcrush,
    pub dropout: Dropout,
}

#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum NoiseKind {
    White,
    Pink,
    Brown,
}

impl NoiseKind {
    pub const ALL: [NoiseKind; 3] = [NoiseKind::White, NoiseKind::Pink, NoiseKind::Brown];
}

impl fmt::Display for NoiseKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            NoiseKind::White => "white",
            NoiseKind::Pink => "pink",
            NoiseKind::Brown => "brown",
        })
    }
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Noise {
    pub kind: NoiseKind,
    /// Noise levels (0.0-2.0) use perceptual (dB-based) scaling:
    /// - 0.0 = silence
    /// - 0.5 = moderate (-30dB)
    /// - 1.0 = reference level (0dB)
    /// - 2.0 = boosted (+18dB)
    pub level: f32,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Bandpass {
    pub low: f32,
    pub high: f32,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Tremolo {
    pub depth: f32,
    pub rate: f32,
    pub jitter: f32,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Clip {
    pub pregain: f32,
    pub threshold: f32,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Bitcrush {
    pub bits: Option<u8>,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Dropout {
    pub probability: f32,
    pub min_ms: f32,
    pub max_ms: f32,
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a * (1.0 - t) + b * t
}

pub fn noise_level(noise: &[Noise], kind: NoiseKind) -> f32 {
    noise
        .iter()
        .find(|layer| layer.kind == kind)
        .map_or(0.0, |layer| layer.level)
}

impl SignalParameters {
    pub fn interpolate(&self, other: &Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        Self {
            name: format!("{}→{}", self.name, other.name),
            noise: interpolate_noise(&self.noise, &other.noise, t),
            bandpass: self.bandpass.interpolate(&other.bandpass, t),
            tremolo: self.tremolo.interpolate(&other.tremolo, t),
            clip: self.clip.interpolate(&other.clip, t),
            bitcrush: self.bitcrush.interpolate(&other.bitcrush, t),
            dropout: self.dropout.interpolate(&other.dropout, t),
        }
    }
}

// The perceptual scale (0.0-2.0) already maps linearly to dB,
// so linear interpolation here gives smooth perceptual transitions.
fn interpolate_noise(from: &[Noise], to: &[Noise], t: f32) -> Vec<Noise> {
    NoiseKind::ALL
        .into_iter()
        .filter(|kind| from.iter().chain(to).any(|layer| layer.kind == *kind))
        .map(|kind| Noise {
            kind,
            level: lerp(noise_level(from, kind), noise_level(to, kind), t),
        })
        .collect()
}

impl Bandpass {
    fn interpolate(&self, other: &Self, t: f32) -> Self {
        Self {
            low: lerp(self.low, other.low, t),
            high: lerp(self.high, other.high, t),
        }
    }
}

impl Tremolo {
    fn interpolate(&self, other: &Self, t: f32) -> Self {
        Self {
            depth: lerp(self.depth, other.depth, t),
            rate: lerp(self.rate, other.rate, t),
            jitter: lerp(self.jitter, other.jitter, t),
        }
    }
}

impl Clip {
    fn interpolate(&self, other: &Self, t: f32) -> Self {
        Self {
            pregain: lerp(self.pregain, other.pregain, t),
            threshold: lerp(self.threshold, other.threshold, t),
        }
    }
}

impl Bitcrush {
    fn interpolate(&self, other: &Self, t: f32) -> Self {
        if t < 0.5 { self.clone() } else { other.clone() }
    }
}

impl Dropout {
    fn interpolate(&self, other: &Self, t: f32) -> Self {
        Self {
            probability: lerp(self.probability, other.probability, t),
            min_ms: lerp(self.min_ms, other.min_ms, t),
            max_ms: lerp(self.max_ms, other.max_ms, t),
        }
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

fn check_noise(noise: &[Noise]) -> anyhow::Result<()> {
    for (index, layer) in noise.iter().enumerate() {
        if noise[..index].iter().any(|seen| seen.kind == layer.kind) {
            bail!("noise kind {} appears more than once", layer.kind);
        }
        check(
            &format!("noise.{}.level", layer.kind),
            layer.level,
            NOISE_LEVEL,
        )?;
    }
    Ok(())
}

impl Bandpass {
    fn check_limits(&self) -> anyhow::Result<()> {
        check("bandpass.low", self.low, BANDPASS_HZ)?;
        check("bandpass.high", self.high, BANDPASS_HZ)?;
        if self.low >= self.high {
            bail!("bandpass.low must be below bandpass.high");
        }
        Ok(())
    }
}

impl Tremolo {
    fn check_limits(&self) -> anyhow::Result<()> {
        check("tremolo.depth", self.depth, UNIT)?;
        check("tremolo.rate", self.rate, TREMOLO_RATE_HZ)?;
        check("tremolo.jitter", self.jitter, UNIT)
    }
}

impl Clip {
    fn check_limits(&self) -> anyhow::Result<()> {
        check("clip.pregain", self.pregain, CLIP_PREGAIN)?;
        check("clip.threshold", self.threshold, CLIP_THRESHOLD)
    }
}

impl Bitcrush {
    fn check_limits(&self) -> anyhow::Result<()> {
        if let Some(bits) = self.bits {
            let (min, max) = BITCRUSH_BITS;
            if !(min..=max).contains(&bits) {
                bail!("bitcrush.bits must be between {min} and {max}, got {bits}");
            }
        }
        Ok(())
    }
}

impl Dropout {
    fn check_limits(&self) -> anyhow::Result<()> {
        check("dropout.probability", self.probability, UNIT)?;
        check("dropout.min_ms", self.min_ms, DROPOUT_MS)?;
        check("dropout.max_ms", self.max_ms, DROPOUT_MS)?;
        if self.min_ms > self.max_ms {
            bail!("dropout.min_ms must not exceed dropout.max_ms");
        }
        Ok(())
    }
}

impl SignalParameters {
    fn check_limits(&self) -> anyhow::Result<()> {
        check_noise(&self.noise)?;
        self.bandpass.check_limits()?;
        self.tremolo.check_limits()?;
        self.clip.check_limits()?;
        self.bitcrush.check_limits()?;
        self.dropout.check_limits()
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
        let cases: [fn(&mut SignalParameters); 7] = [
            |p| p.bandpass.high = 24000.0,
            |p| p.bandpass.low = 20000.0,
            |p| p.bandpass.low = 0.0,
            |p| (p.dropout.min_ms, p.dropout.max_ms) = (100.0, 50.0),
            |p| p.bitcrush.bits = Some(255),
            |p| p.noise = vec![pink(2.5)],
            |p| p.noise = vec![pink(0.5), pink(0.5)],
        ];
        for mutate in cases {
            let mut preset = clear();
            mutate(&mut preset);
            assert!(preset.validate().is_err());
        }
    }

    fn pink(level: f32) -> Noise {
        Noise {
            kind: NoiseKind::Pink,
            level,
        }
    }

    #[test]
    fn interpolate_treats_a_missing_noise_kind_as_silent() {
        let from = clear();
        let mut to = clear();
        to.noise = vec![pink(1.0)];
        let halfway = from.interpolate(&to, 0.5);
        assert_eq!(halfway.noise.len(), 1);
        assert_eq!(noise_level(&halfway.noise, NoiseKind::Pink), 0.5);
        assert_eq!(noise_level(&halfway.noise, NoiseKind::White), 0.0);
    }
}
