use crate::audio::dj::segments::{
    HexMessageDefaults, HexMessageEntry, HexSegment, NoisePeriodEntry, NoiseSegment, Segment,
    TrackEntry, TrackSegment, ensure_loops,
};
use crate::audio::dj::weighted_choice::{Weighted, WeightedPool};
use crate::bucket::registry::RegistryEntry;
use crate::state::Data;
use anyhow::{Context, bail};
use field_trait::field_trait_impl;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

#[derive(Deserialize, Serialize, Debug, Default)]
pub struct DJConfig {
    pub name: String,
    pub pools: Vec<Pool>,
    pub signal_profiles: SignalProfiles,
    pub channel_status: Option<String>,
}

#[derive(Deserialize, Serialize, Debug, Default)]
pub struct SignalProfiles {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub history_penalty_multiplier: Option<f32>,
    pub entries: Vec<SignalProfileEntry>,
}

#[derive(Deserialize, Serialize, Debug)]
pub struct Pool {
    pub name: String,
    pub weight: u32,
    pub hex_defaults: Option<HexMessageDefaults>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub history_penalty_multiplier: Option<f32>,
    pub entries: Vec<PoolEntry>,
}

impl WeightedPool<SignalProfileEntry> for SignalProfiles {
    fn entries(&self) -> &Vec<SignalProfileEntry> {
        &self.entries
    }

    fn history_penalty_multiplier(&self) -> Option<f32> {
        self.history_penalty_multiplier
    }
}

impl WeightedPool<PoolEntry> for Pool {
    fn entries(&self) -> &Vec<PoolEntry> {
        &self.entries
    }

    fn history_penalty_multiplier(&self) -> Option<f32> {
        self.history_penalty_multiplier
    }
}

static BUILT_IN_HEX_DEFAULTS: LazyLock<HexMessageDefaults> = LazyLock::new(Default::default);

impl Pool {
    pub fn hex_defaults(&self) -> &HexMessageDefaults {
        self.hex_defaults.as_ref().unwrap_or(&BUILT_IN_HEX_DEFAULTS)
    }

    fn validate(&self) -> anyhow::Result<()> {
        let defaults = self.hex_defaults();
        ensure_loops(defaults.loop_min, defaults.loop_max).context("hex_defaults")?;
        ensure_penalty_multiplier(self.history_penalty_multiplier)?;
        self.entries
            .iter()
            .try_for_each(|entry| entry.validate(defaults))
    }
}

/// The kind is inferred from which fields are present.
#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(untagged)]
#[field_trait_impl(Weighted)]
pub enum PoolEntry {
    Track(TrackEntry),
    Hex(HexMessageEntry),
    Noise(NoisePeriodEntry),
}

impl PoolEntry {
    fn validate(&self, hex_defaults: &HexMessageDefaults) -> anyhow::Result<()> {
        match self {
            PoolEntry::Track(track) => {
                if let Some(seconds) = track.max_duration_seconds {
                    ensure_seconds(seconds, || format!("track {:?}", track.filename))?;
                }
            }
            PoolEntry::Hex(message) => ensure_loops(
                message.loop_min.unwrap_or(hex_defaults.loop_min),
                message.loop_max.unwrap_or(hex_defaults.loop_max),
            )
            .with_context(|| format!("hex message {:?}", message.text))?,
            PoolEntry::Noise(noise) => {
                let context = || format!("noise period {:?}", noise.noise_profile);
                ensure_seconds(noise.min_duration_seconds, context)?;
                ensure_seconds(noise.max_duration_seconds, context)?;
            }
        }
        Ok(())
    }

    pub async fn segment(
        &self,
        hex_defaults: &HexMessageDefaults,
        bot_state: &Data,
    ) -> Box<dyn Segment> {
        match self {
            PoolEntry::Track(track) => {
                Box::new(TrackSegment::from_entry(track.clone(), &bot_state.duration_cache).await)
            }
            PoolEntry::Hex(message) => {
                Box::new(HexSegment::from_entry(message.clone(), hex_defaults))
            }
            PoolEntry::Noise(noise) => Box::new(NoiseSegment::from(noise)),
        }
    }
}

#[field_trait_impl(Weighted)]
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct SignalProfileEntry {
    pub profile_name: String,
    pub weight: u32,
    pub fade_duration_seconds: f32,
    pub min_time_seconds: f32,
    pub max_time_seconds: f32,
}

impl RegistryEntry for DJConfig {
    const KIND: &'static str = "DJ config";

    fn name(&self) -> &str {
        &self.name
    }

    /// Rejects values that would make the DJ panic when it picks them.
    fn validate(&self) -> anyhow::Result<()> {
        for pool in &self.pools {
            pool.validate()
                .with_context(|| format!("pool {:?}", pool.name))?;
        }
        ensure_penalty_multiplier(self.signal_profiles.history_penalty_multiplier)
            .context("signal_profiles")?;
        for profile in &self.signal_profiles.entries {
            let context = || format!("signal profile {:?}", profile.profile_name);
            ensure_seconds(profile.fade_duration_seconds, context)?;
            ensure_seconds(profile.min_time_seconds, context)?;
            if profile.min_time_seconds >= profile.max_time_seconds {
                bail!(
                    "{}: min_time_seconds must be less than max_time_seconds",
                    context()
                );
            }
        }
        Ok(())
    }
}

fn ensure_penalty_multiplier(multiplier: Option<f32>) -> anyhow::Result<()> {
    if let Some(multiplier) = multiplier
        && !(0.0..=1.0).contains(&multiplier)
    {
        bail!("history_penalty_multiplier {multiplier} is not between 0 and 1");
    }
    Ok(())
}

pub fn ensure_seconds(seconds: f32, context: impl Fn() -> String) -> anyhow::Result<()> {
    if !(seconds.is_finite() && seconds >= 0.0) {
        bail!(
            "{}: {seconds} is not a non-negative number of seconds",
            context()
        );
    }
    Ok(())
}
