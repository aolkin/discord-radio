use crate::audio::dj::segments::{
    HexMessageDefaults, HexMessageEntry, HexSegment, NoisePeriodEntry, NoiseSegment, Segment,
    TrackEntry, TrackSegment, ensure_loops,
};
use crate::audio::dj::weighted_choice::Weighted;
use crate::bucket::registry::RegistryEntry;
use crate::state::Data;
use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::sync::LazyLock;

#[derive(Deserialize, Serialize, Debug, Default)]
pub struct DJConfig {
    pub name: String,
    pub pools: Vec<Pool>,
    pub signal_profiles: Vec<SignalProfileEntry>,
    pub recent_history_size: usize,
    pub duplicate_penalty_multiplier: f32,
    pub channel_status: Option<String>,
}

#[derive(Deserialize, Serialize, Debug)]
pub struct Pool {
    pub name: String,
    pub weight: u32,
    pub hex_defaults: Option<HexMessageDefaults>,
    pub entries: Vec<PoolEntry>,
}

static BUILT_IN_HEX_DEFAULTS: LazyLock<HexMessageDefaults> = LazyLock::new(Default::default);

impl Pool {
    pub fn hex_defaults(&self) -> &HexMessageDefaults {
        self.hex_defaults.as_ref().unwrap_or(&BUILT_IN_HEX_DEFAULTS)
    }

    fn validate(&self) -> anyhow::Result<()> {
        let defaults = self.hex_defaults();
        ensure_loops(defaults.loop_min, defaults.loop_max).context("hex_defaults")?;
        self.entries
            .iter()
            .try_for_each(|entry| entry.validate(defaults))
    }
}

impl Weighted for Pool {
    fn weight(&self) -> f32 {
        if self.entries.is_empty() {
            0.0
        } else {
            self.weight as f32
        }
    }

    fn key(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.name)
    }
}

/// The kind is inferred from which fields are present.
#[derive(Deserialize, Serialize, Debug)]
#[serde(untagged)]
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

impl Weighted for PoolEntry {
    fn weight(&self) -> f32 {
        match self {
            PoolEntry::Track(track) => track.weight as f32,
            PoolEntry::Hex(message) => message.weight as f32,
            PoolEntry::Noise(noise) => noise.weight as f32,
        }
    }

    fn key(&self) -> Cow<'_, str> {
        match self {
            PoolEntry::Track(track) => Cow::Owned(format!("track:{}", track.filename)),
            PoolEntry::Hex(message) => Cow::Owned(format!("hex:{}", message.text)),
            PoolEntry::Noise(noise) => Cow::Owned(format!("noise:{}", noise.noise_profile)),
        }
    }
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct SignalProfileEntry {
    pub profile_name: String,
    pub weight: u32,
    pub fade_duration_seconds: f32,
    pub min_time_seconds: f32,
    pub max_time_seconds: f32,
}

impl Weighted for SignalProfileEntry {
    fn weight(&self) -> f32 {
        self.weight as f32
    }

    fn key(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.profile_name)
    }
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
        for profile in &self.signal_profiles {
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

pub fn ensure_seconds(seconds: f32, context: impl Fn() -> String) -> anyhow::Result<()> {
    if !(seconds.is_finite() && seconds >= 0.0) {
        bail!(
            "{}: {seconds} is not a non-negative number of seconds",
            context()
        );
    }
    Ok(())
}
