use crate::audio::dsp::effects::bitcrush::BitcrushParams;
use crate::audio::dsp::effects::clip::ClipParams;
use crate::audio::dsp::effects::dropout::DropoutParams;
use crate::audio::dsp::effects::noise::NoiseParams;
use crate::audio::dsp::effects::tremolo::TremoloParams;
use crate::audio::dsp::filter::{FILTER_HZ, FilterParams};
use crate::bucket::registry::{JsonRegistry, RegistryEntry, is_valid_name};
use anyhow::{Context, anyhow, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::HashSet;

#[derive(Deserialize, Serialize, Clone, Debug, garde::Validate)]
pub struct SignalParameters {
    #[garde(skip)]
    pub name: String,
    #[garde(
        length(max = 16),
        custom(unique_names),
        dive(FilterBounds::of(&self.effects))
    )]
    pub effects: Vec<EffectEntry>,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, garde::Validate)]
#[garde(context(FilterBounds))]
pub struct EffectEntry {
    #[garde(length(min = 1, max = 32), custom(registry_name))]
    pub name: String,
    #[serde(flatten)]
    #[garde(dive)]
    pub effect: EffectParams,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, garde::Validate)]
#[serde(tag = "kind", content = "params", rename_all = "snake_case")]
#[garde(context(FilterBounds))]
pub enum EffectParams {
    Noise(#[garde(dive(()))] NoiseParams),
    Highpass(#[garde(dive(()), custom(below_lowest_lowpass))] FilterParams),
    Lowpass(#[garde(dive(()))] FilterParams),
    Tremolo(#[garde(dive(()))] TremoloParams),
    Clip(#[garde(dive(()))] ClipParams),
    Bitcrush(#[garde(dive(()))] BitcrushParams),
    Dropout(#[garde(dive(()))] DropoutParams),
}

pub struct FilterBounds {
    lowest_lowpass: f32,
}

impl FilterBounds {
    fn of(effects: &[EffectEntry]) -> Self {
        let lowest_lowpass = effects
            .iter()
            .filter_map(|entry| match &entry.effect {
                EffectParams::Lowpass(filter) => Some(filter.frequency),
                _ => None,
            })
            .fold(f32::INFINITY, f32::min);
        Self { lowest_lowpass }
    }
}

fn below_lowest_lowpass(filter: &FilterParams, bounds: &FilterBounds) -> garde::Result {
    if filter.frequency >= bounds.lowest_lowpass {
        return Err(garde::Error::new(format!(
            "not below the lowest lowpass frequency ({})",
            bounds.lowest_lowpass
        )));
    }
    Ok(())
}

fn registry_name(name: &str, _: &FilterBounds) -> garde::Result {
    if !is_valid_name(name) {
        return Err(garde::Error::new(
            "may only contain letters, digits, '-' and '_'",
        ));
    }
    Ok(())
}

fn unique_names(effects: &[EffectEntry], _: &()) -> garde::Result {
    let mut names = HashSet::new();
    match effects.iter().find(|entry| !names.insert(&entry.name)) {
        Some(entry) => Err(garde::Error::new(format!(
            "effect name `{}` is used more than once",
            entry.name
        ))),
        None => Ok(()),
    }
}

pub trait Lerp {
    fn lerp(&self, other: &Self, t: f32) -> Self;
}

impl Lerp for f32 {
    fn lerp(&self, other: &Self, t: f32) -> Self {
        self * (1.0 - t) + other * t
    }
}

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

impl EffectParams {
    fn neutral(&self) -> Self {
        match *self {
            Self::Noise(noise) => Self::Noise(NoiseParams {
                level: 0.0,
                ..noise
            }),
            Self::Highpass(_) => Self::Highpass(FilterParams {
                frequency: FILTER_HZ.0,
            }),
            Self::Lowpass(_) => Self::Lowpass(FilterParams {
                frequency: FILTER_HZ.1,
            }),
            Self::Tremolo(tremolo) => Self::Tremolo(TremoloParams {
                depth: 0.0,
                ..tremolo
            }),
            Self::Clip(_) => Self::Clip(ClipParams {
                pregain: 1.0,
                threshold: 2.0,
            }),
            Self::Bitcrush(_) => Self::Bitcrush(BitcrushParams { bits: 16 }),
            Self::Dropout(dropout) => Self::Dropout(DropoutParams {
                probability: 0.0,
                ..dropout
            }),
        }
    }

    fn lerp(&self, other: &Self, t: f32) -> Option<Self> {
        Some(match (self, other) {
            (Self::Noise(a), Self::Noise(b)) if a.color == b.color => Self::Noise(a.lerp(b, t)),
            (Self::Highpass(a), Self::Highpass(b)) => Self::Highpass(a.lerp(b, t)),
            (Self::Lowpass(a), Self::Lowpass(b)) => Self::Lowpass(a.lerp(b, t)),
            (Self::Tremolo(a), Self::Tremolo(b)) => Self::Tremolo(a.lerp(b, t)),
            (Self::Clip(a), Self::Clip(b)) => Self::Clip(a.lerp(b, t)),
            (Self::Bitcrush(a), Self::Bitcrush(b)) => Self::Bitcrush(a.lerp(b, t)),
            (Self::Dropout(a), Self::Dropout(b)) => Self::Dropout(a.lerp(b, t)),
            _ => return None,
        })
    }

    fn fade_in(&self, t: f32) -> Self {
        self.neutral()
            .lerp(self, t)
            .expect("neutral params keep the effect's kind")
    }
}

impl EffectEntry {
    fn with_effect(&self, effect: EffectParams) -> Self {
        Self {
            name: self.name.clone(),
            effect,
        }
    }
}

impl SignalParameters {
    pub fn interpolate(&self, other: &Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        let mut unmatched: Vec<Option<&EffectEntry>> = self.effects.iter().map(Some).collect();
        let mut effects: Vec<(Option<usize>, EffectEntry)> = other
            .effects
            .iter()
            .map(|to| {
                let matched = unmatched.iter_mut().enumerate().find_map(|(index, from)| {
                    let effect = from
                        .filter(|from| from.name == to.name)?
                        .effect
                        .lerp(&to.effect, t)?;
                    *from = None;
                    Some((index, effect))
                });
                match matched {
                    Some((index, effect)) => (Some(index), to.with_effect(effect)),
                    None => (None, to.with_effect(to.effect.fade_in(t))),
                }
            })
            .collect();
        for (index, from) in unmatched.into_iter().enumerate() {
            let Some(from) = from else { continue };
            let position = match index.checked_sub(1) {
                Some(previous) => effects
                    .iter()
                    .position(|(source, _)| *source == Some(previous))
                    .map_or(0, |position| position + 1),
                None => 0,
            };
            effects.insert(
                position,
                (Some(index), from.with_effect(from.effect.fade_in(1.0 - t))),
            );
        }
        Self {
            name: format!("{}→{}", self.name, other.name),
            effects: effects.into_iter().map(|(_, entry)| entry).collect(),
        }
    }

    pub fn with_effect_added(&self, index: usize, effect: EffectEntry) -> anyhow::Result<Self> {
        self.edited(|effects| {
            if index > effects.len() {
                bail!("index {index} is past the end of the effects list");
            }
            effects.insert(index, effect);
            Ok(())
        })
    }

    pub fn with_params_edited(
        &self,
        name: &str,
        params: Map<String, Value>,
    ) -> anyhow::Result<Self> {
        self.edited(|effects| {
            let index = position(effects, name)?;
            let entry = &mut effects[index];
            let mut effect = serde_json::to_value(&entry.effect)?;
            let fields = effect
                .get_mut("params")
                .and_then(Value::as_object_mut)
                .with_context(|| format!("effect {name:?} has no params object"))?;
            for (field, value) in params {
                if !fields.contains_key(&field) {
                    bail!("{field:?} is not a param of effect {name:?}");
                }
                fields.insert(field, value);
            }
            entry.effect = serde_json::from_value(effect)
                .with_context(|| format!("reading the edited params of effect {name:?}"))?;
            Ok(())
        })
    }

    pub fn without_effect(&self, name: &str) -> anyhow::Result<Self> {
        self.edited(|effects| {
            effects.remove(position(effects, name)?);
            Ok(())
        })
    }

    pub fn with_effect_moved(&self, name: &str, index: usize) -> anyhow::Result<Self> {
        self.edited(|effects| {
            if index >= effects.len() {
                bail!("index {index} is past the end of the effects list");
            }
            let entry = effects.remove(position(effects, name)?);
            effects.insert(index, entry);
            Ok(())
        })
    }

    fn edited(
        &self,
        edit: impl FnOnce(&mut Vec<EffectEntry>) -> anyhow::Result<()>,
    ) -> anyhow::Result<Self> {
        let mut edited = self.clone();
        edit(&mut edited.effects)?;
        edited.validate()?;
        Ok(edited)
    }
}

fn position(effects: &[EffectEntry], name: &str) -> anyhow::Result<usize> {
    effects
        .iter()
        .position(|entry| entry.name == name)
        .with_context(|| format!("no effect is named {name:?}"))
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
    use crate::audio::dsp::effects::noise::NoiseColor;

    fn entry(name: &str, effect: EffectParams) -> EffectEntry {
        EffectEntry {
            name: name.to_owned(),
            effect,
        }
    }

    fn preset(effects: Vec<EffectEntry>) -> SignalParameters {
        SignalParameters {
            name: "test".to_owned(),
            effects,
        }
    }

    fn noise(color: NoiseColor, level: f32) -> EffectParams {
        EffectParams::Noise(NoiseParams { color, level })
    }

    fn highpass(frequency: f32) -> EffectParams {
        EffectParams::Highpass(FilterParams { frequency })
    }

    fn lowpass(frequency: f32) -> EffectParams {
        EffectParams::Lowpass(FilterParams { frequency })
    }

    #[test]
    fn validate_rejects_values_that_break_the_dsp() {
        let weak: SignalParameters =
            serde_json::from_str(include_str!("default_presets/weak_signal.json")).unwrap();
        assert!(weak.validate().is_ok());
        let pink = || noise(NoiseColor::Pink, 0.5);
        let cases = [
            vec![entry("lowpass", lowpass(24000.0))],
            vec![entry("highpass", highpass(0.0))],
            vec![
                entry("highpass", highpass(5000.0)),
                entry("lowpass", lowpass(500.0)),
            ],
            vec![
                entry("highpass", highpass(100.0)),
                entry("lowpass", lowpass(4000.0)),
                entry("highpass_2", highpass(5000.0)),
            ],
            vec![entry(
                "dropout",
                EffectParams::Dropout(DropoutParams {
                    probability: 0.5,
                    min_ms: 100.0,
                    max_ms: 50.0,
                }),
            )],
            vec![entry(
                "bitcrush",
                EffectParams::Bitcrush(BitcrushParams { bits: 0 }),
            )],
            vec![entry("noise", noise(NoiseColor::Pink, 2.5))],
            vec![entry("noise", pink()), entry("noise", pink())],
            vec![entry("bad name", pink())],
            vec![entry("", pink())],
            (0..17)
                .map(|index| entry(&format!("noise{index}"), pink()))
                .collect(),
        ];
        for effects in cases {
            assert!(preset(effects).validate().is_err());
        }
    }

    #[test]
    fn interpolate_pairs_effects_by_name() {
        let from = preset(vec![
            entry("hiss", noise(NoiseColor::Pink, 1.0)),
            entry("gone", highpass(220.0)),
            entry("filter", lowpass(4000.0)),
        ]);
        let to = preset(vec![
            entry("filter", lowpass(2000.0)),
            entry("hiss", noise(NoiseColor::White, 0.5)),
            entry("new", highpass(420.0)),
        ]);
        assert_eq!(
            from.interpolate(&to, 0.5).effects,
            vec![
                entry("hiss", noise(NoiseColor::Pink, 0.5)),
                entry("gone", highpass(120.0)),
                entry("filter", lowpass(3000.0)),
                entry("hiss", noise(NoiseColor::White, 0.25)),
                entry("new", highpass(220.0)),
            ]
        );
    }

    #[test]
    fn live_edits_target_effects_by_name() {
        let base = preset(vec![
            entry("hiss", noise(NoiseColor::Pink, 1.0)),
            entry("filter", lowpass(4000.0)),
        ]);
        let params = |value: Value| value.as_object().unwrap().clone();
        let edited = base
            .with_effect_added(1, entry("cut", highpass(200.0)))
            .unwrap()
            .with_params_edited("hiss", params(serde_json::json!({ "level": 0.5 })))
            .unwrap()
            .with_effect_moved("hiss", 2)
            .unwrap()
            .without_effect("filter")
            .unwrap();
        assert_eq!(
            edited.effects,
            vec![
                entry("cut", highpass(200.0)),
                entry("hiss", noise(NoiseColor::Pink, 0.5)),
            ]
        );
        assert!(
            base.with_effect_added(3, entry("cut", highpass(200.0)))
                .is_err()
        );
        assert!(
            base.with_effect_added(0, entry("hiss", highpass(200.0)))
                .is_err()
        );
        assert!(
            base.with_effect_added(0, entry("cut", highpass(5000.0)))
                .is_err()
        );
        assert!(
            base.with_params_edited("hiss", params(serde_json::json!({ "level": 2.5 })))
                .is_err()
        );
        assert!(
            base.with_params_edited("hiss", params(serde_json::json!({ "depth": 0.5 })))
                .is_err()
        );
        assert!(
            base.with_params_edited("missing", params(serde_json::json!({})))
                .is_err()
        );
        assert!(base.with_effect_moved("hiss", 2).is_err());
        assert!(base.without_effect("missing").is_err());
    }
}
