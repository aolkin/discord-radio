use crate::audio::dsp::effects::bitcrush::Bitcrush;
use crate::audio::dsp::effects::clip::Clip;
use crate::audio::dsp::effects::dropout::Dropout;
use crate::audio::dsp::effects::filter::{Filter, highpass_below_lowpass};
use crate::audio::dsp::effects::noise::Noise;
use crate::audio::dsp::effects::tremolo::Tremolo;
use crate::bucket::registry::{JsonRegistry, RegistryEntry};
use anyhow::anyhow;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Clone, Debug, garde::Validate)]
pub struct SignalParameters {
    #[garde(skip)]
    pub name: String,
    #[garde(dive)]
    pub noise: Vec<Noise>,
    #[garde(dive, custom(highpass_below_lowpass))]
    pub filters: Vec<Filter>,
    #[garde(dive)]
    pub tremolo: Tremolo,
    #[garde(dive)]
    pub clip: Clip,
    #[garde(dive)]
    pub bitcrush: Option<Bitcrush>,
    #[garde(dive)]
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
        impl $crate::audio::presets::Lerp for $type {
            fn lerp(&self, other: &Self, t: f32) -> Self {
                if t < 0.5 { *self } else { *other }
            }
        }
    )*};
}

pub(crate) use lerp_by_step;

macro_rules! lerp_struct {
    (
        $(#[$meta:meta])*
        pub struct $name:ident { $($(#[$field_meta:meta])* pub $field:ident: $type:ty,)* }
    ) => {
        $(#[$meta])*
        pub struct $name { $($(#[$field_meta])* pub $field: $type,)* }

        impl $crate::audio::presets::Lerp for $name {
            fn lerp(&self, other: &Self, t: f32) -> Self {
                Self { $($field: self.$field.lerp(&other.$field, t),)* }
            }
        }
    };
}

pub(crate) use lerp_struct;

pub trait ListEntry: Lerp + Copy {
    type Kind: Copy + PartialEq;

    fn kind(&self) -> Self::Kind;
    fn neutral(kind: Self::Kind) -> Self;
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

pub fn not_below(name: &'static str, min: f32) -> impl FnOnce(&f32, &()) -> garde::Result {
    move |value, _| {
        if *value < min {
            return Err(garde::Error::new(format!("lower than {name} ({min})")));
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
        garde::Validate::validate(self)
            .map_err(|report| anyhow!(report.to_string().trim_end().to_owned()))
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
    use crate::audio::dsp::effects::filter::FilterKind;
    use crate::audio::dsp::effects::noise::NoiseKind;

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
