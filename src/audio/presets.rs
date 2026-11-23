use crate::bucket::registry::{JsonRegistry, RegistryEntry};
use anyhow::bail;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct SignalParameters {
    pub name: String,
    pub noise: Vec<Noise>,
    pub filters: Vec<Filter>,
    pub tremolo: Tremolo,
    pub clip: Clip,
    pub bitcrush: Option<Bitcrush>,
    pub dropout: Dropout,
}

pub trait Lerp {
    fn lerp(&self, other: &Self, t: f32) -> Self;
}

impl Lerp for f32 {
    fn lerp(&self, other: &Self, t: f32) -> Self {
        self * (1.0 - t) + other * t
    }
}

macro_rules! lerp_by_step {
    ($($type:ty),*) => {$(
        impl Lerp for $type {
            fn lerp(&self, other: &Self, t: f32) -> Self {
                if t < 0.5 { *self } else { *other }
            }
        }
    )*};
}

lerp_by_step!(NoiseKind, FilterKind, Option<Bitcrush>);

macro_rules! lerp_struct {
    (
        $(#[$meta:meta])*
        pub struct $name:ident { $($(#[$field_meta:meta])* pub $field:ident: $type:ty,)* }
    ) => {
        $(#[$meta])*
        pub struct $name { $($(#[$field_meta])* pub $field: $type,)* }

        impl Lerp for $name {
            fn lerp(&self, other: &Self, t: f32) -> Self {
                Self { $($field: self.$field.lerp(&other.$field, t),)* }
            }
        }
    };
}

#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq, Eq, strum::Display)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum NoiseKind {
    White,
    Pink,
    Brown,
}

lerp_struct! {
    #[derive(Deserialize, Serialize, Clone, Copy, Debug)]
    pub struct Noise {
        pub kind: NoiseKind,
        /// Noise levels (0.0-2.0) use perceptual (dB-based) scaling:
        /// - 0.0 = silence
        /// - 0.5 = moderate (-30dB)
        /// - 1.0 = reference level (0dB)
        /// - 2.0 = boosted (+18dB)
        pub level: f32,
    }
}

#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq, Eq, strum::Display)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum FilterKind {
    HighPass,
    LowPass,
}

lerp_struct! {
    #[derive(Deserialize, Serialize, Clone, Copy, Debug)]
    pub struct Filter {
        pub kind: FilterKind,
        pub frequency: f32,
    }
}

lerp_struct! {
    #[derive(Deserialize, Serialize, Clone, Copy, Debug)]
    pub struct Tremolo {
        pub depth: f32,
        pub rate: f32,
        pub jitter: f32,
    }
}

lerp_struct! {
    #[derive(Deserialize, Serialize, Clone, Copy, Debug)]
    pub struct Clip {
        pub pregain: f32,
        pub threshold: f32,
    }
}

#[derive(Deserialize, Serialize, Clone, Copy, Debug)]
pub struct Bitcrush {
    pub bits: u8,
}

lerp_struct! {
    #[derive(Deserialize, Serialize, Clone, Copy, Debug)]
    pub struct Dropout {
        pub probability: f32,
        pub min_ms: f32,
        pub max_ms: f32,
    }
}

pub trait ListEntry: Lerp + Copy {
    type Kind: Copy + PartialEq;
    const LIST: &'static str;

    fn kind(&self) -> Self::Kind;
    fn neutral(kind: Self::Kind) -> Self;
    fn check_limits(&self, path: &str) -> anyhow::Result<()>;
}

impl ListEntry for Noise {
    type Kind = NoiseKind;
    const LIST: &'static str = "noise";

    fn kind(&self) -> NoiseKind {
        self.kind
    }

    fn neutral(kind: NoiseKind) -> Self {
        Self { kind, level: 0.0 }
    }

    fn check_limits(&self, path: &str) -> anyhow::Result<()> {
        check(&format!("{path}.level"), self.level, NOISE_LEVEL)
    }
}

impl ListEntry for Filter {
    type Kind = FilterKind;
    const LIST: &'static str = "filters";

    fn kind(&self) -> FilterKind {
        self.kind
    }

    fn neutral(kind: FilterKind) -> Self {
        let frequency = match kind {
            FilterKind::HighPass => FILTER_HZ.0,
            FilterKind::LowPass => FILTER_HZ.1,
        };
        Self { kind, frequency }
    }

    fn check_limits(&self, path: &str) -> anyhow::Result<()> {
        check(&format!("{path}.frequency"), self.frequency, FILTER_HZ)
    }
}

impl<E: ListEntry> Lerp for Vec<E> {
    fn lerp(&self, other: &Self, t: f32) -> Self {
        let mut entries = Vec::new();
        for index in 0..self.len().max(other.len()) {
            match (self.get(index), other.get(index)) {
                (Some(from), Some(to)) if from.kind() == to.kind() => {
                    entries.push(from.lerp(to, t));
                }
                (from, to) => {
                    entries.extend(from.map(|from| from.lerp(&E::neutral(from.kind()), t)));
                    entries.extend(to.map(|to| E::neutral(to.kind()).lerp(to, t)));
                }
            }
        }
        entries
    }
}

impl SignalParameters {
    pub fn interpolate(&self, other: &Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        Self {
            name: format!("{}→{}", self.name, other.name),
            noise: self.noise.lerp(&other.noise, t),
            filters: self.filters.lerp(&other.filters, t),
            tremolo: self.tremolo.lerp(&other.tremolo, t),
            clip: self.clip.lerp(&other.clip, t),
            bitcrush: self.bitcrush.lerp(&other.bitcrush, t),
            dropout: self.dropout.lerp(&other.dropout, t),
        }
    }
}

const FILTER_HZ: (f32, f32) = (20.0, 20000.0);
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

fn check_entries<E: ListEntry>(entries: &[E]) -> anyhow::Result<()> {
    for (index, entry) in entries.iter().enumerate() {
        entry.check_limits(&format!("{}[{index}]", E::LIST))?;
    }
    Ok(())
}

fn check_filters(filters: &[Filter]) -> anyhow::Result<()> {
    check_entries(filters)?;
    let frequencies = |kind| {
        filters
            .iter()
            .filter(move |filter| filter.kind == kind)
            .map(|filter| filter.frequency)
    };
    let lowest_lowpass = frequencies(FilterKind::LowPass).fold(f32::INFINITY, f32::min);
    if frequencies(FilterKind::HighPass).any(|highpass| highpass >= lowest_lowpass) {
        bail!("every highpass frequency must be below every lowpass frequency");
    }
    Ok(())
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
        let (min, max) = BITCRUSH_BITS;
        if !(min..=max).contains(&self.bits) {
            bail!(
                "bitcrush.bits must be between {min} and {max}, got {}",
                self.bits
            );
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
        check_entries(&self.noise)?;
        check_filters(&self.filters)?;
        self.tremolo.check_limits()?;
        self.clip.check_limits()?;
        if let Some(bitcrush) = &self.bitcrush {
            bitcrush.check_limits()?;
        }
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

    fn filter(kind: FilterKind, frequency: f32) -> Filter {
        Filter { kind, frequency }
    }

    fn pink(level: f32) -> Noise {
        Noise {
            kind: NoiseKind::Pink,
            level,
        }
    }

    #[test]
    fn validate_rejects_values_that_break_the_dsp() {
        assert!(clear().validate().is_ok());
        let cases: [fn(&mut SignalParameters); 8] = [
            |p| p.filters = vec![filter(FilterKind::LowPass, 24000.0)],
            |p| p.filters = vec![filter(FilterKind::HighPass, 0.0)],
            |p| {
                p.filters = vec![
                    filter(FilterKind::HighPass, 5000.0),
                    filter(FilterKind::LowPass, 500.0),
                ]
            },
            |p| (p.dropout.min_ms, p.dropout.max_ms) = (100.0, 50.0),
            |p| p.bitcrush = Some(Bitcrush { bits: 255 }),
            |p| p.bitcrush = Some(Bitcrush { bits: 0 }),
            |p| {
                p.filters = vec![
                    filter(FilterKind::HighPass, 100.0),
                    filter(FilterKind::LowPass, 4000.0),
                    filter(FilterKind::HighPass, 5000.0),
                ]
            },
            |p| p.noise = vec![pink(0.5), pink(2.5)],
        ];
        for mutate in cases {
            let mut preset = clear();
            mutate(&mut preset);
            assert!(preset.validate().is_err());
        }
    }

    #[test]
    fn interpolate_treats_a_missing_entry_as_neutral() {
        let from = clear();
        let mut to = clear();
        to.noise = vec![pink(1.0)];
        to.filters = vec![filter(FilterKind::HighPass, 220.0)];
        let halfway = from.interpolate(&to, 0.5);
        assert_eq!(halfway.noise.len(), 1);
        assert_eq!(halfway.noise[0].level, 0.5);
        assert_eq!(halfway.filters.len(), 1);
        assert_eq!(halfway.filters[0].frequency, 120.0);
    }
}
