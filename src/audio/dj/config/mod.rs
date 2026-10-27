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
use std::sync::{Arc, LazyLock};

#[derive(Deserialize, Serialize, Debug, Default)]
pub struct DJConfig {
    pub name: String,
    pub pools: Arc<[Arc<Pool>]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signal_profiles: Option<SignalProfiles>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub playback_config: Option<PlaybackConfig>,
}

impl DJConfig {
    pub fn playback_config(&self) -> &PlaybackConfig {
        self.playback_config.as_ref().unwrap_or(&NO_PLAYBACK_CONFIG)
    }

    /// The settings for a segment from `pool`, or from no pool.
    pub fn segment_playback_config(&self, pool: Option<&Pool>) -> PlaybackConfig {
        pool.and_then(|pool| pool.playback_config.as_ref())
            .unwrap_or(&NO_PLAYBACK_CONFIG)
            .with_defaults(self.playback_config())
    }
}

#[field_trait_impl(WeightedPool<SignalProfileEntry>)]
#[derive(Deserialize, Serialize, Debug, Default)]
pub struct SignalProfiles {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub history_penalty_multiplier: Option<f32>,
    pub entries: Arc<[SignalProfileEntry]>,
}

#[derive(Deserialize, Serialize, Clone, Debug, Default)]
pub struct PlaybackConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub history_penalty_multiplier: Option<f32>,
    /// On a pool, the profile for its track and hex message segments whose
    /// entry names none. On the DJ config, the profile the rotation plays when
    /// it has no entry to pick.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signal_profile: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel_status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hex_defaults: Option<HexMessageDefaults>,
}

static NO_PLAYBACK_CONFIG: PlaybackConfig = PlaybackConfig {
    history_penalty_multiplier: None,
    signal_profile: None,
    channel_status: None,
    hex_defaults: None,
};

static BUILT_IN_HEX_DEFAULTS: LazyLock<HexMessageDefaults> = LazyLock::new(Default::default);

impl PlaybackConfig {
    /// Fills unset settings from `defaults`, except `signal_profile`, which
    /// means something different on the DJ config.
    pub fn with_defaults(&self, defaults: &Self) -> Self {
        Self {
            history_penalty_multiplier: self
                .history_penalty_multiplier
                .or(defaults.history_penalty_multiplier),
            signal_profile: self.signal_profile.clone(),
            channel_status: self
                .channel_status
                .clone()
                .or_else(|| defaults.channel_status.clone()),
            hex_defaults: self
                .hex_defaults
                .clone()
                .or_else(|| defaults.hex_defaults.clone()),
        }
    }

    pub fn hex_defaults(&self) -> &HexMessageDefaults {
        self.hex_defaults.as_ref().unwrap_or(&BUILT_IN_HEX_DEFAULTS)
    }

    fn validate(&self) -> anyhow::Result<()> {
        ensure_penalty_multiplier(self.history_penalty_multiplier)?;
        if let Some(defaults) = &self.hex_defaults {
            ensure_loops(defaults.loop_min, defaults.loop_max).context("hex_defaults")?;
        }
        Ok(())
    }
}

#[derive(Deserialize, Serialize, Debug)]
pub struct Pool {
    pub name: String,
    pub weight: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub playback_config: Option<PlaybackConfig>,
    pub entries: Arc<[PoolEntry]>,
}

impl Pool {
    pub fn has_weighted_entry(&self) -> bool {
        self.entries.iter().any(|entry| entry.weight() > 0)
    }

    fn validate(&self, playback: &PlaybackConfig) -> anyhow::Result<()> {
        if let Some(own) = &self.playback_config {
            own.validate().context("playback_config")?;
        }
        if !self.has_weighted_entry() && self.weight != 0 {
            bail!("a pool with no entry of weight above 0 must have weight 0");
        }
        let hex_defaults = playback.hex_defaults();
        self.entries
            .iter()
            .try_for_each(|entry| entry.validate(hex_defaults))
    }
}

/// The kind is inferred from which fields are present.
#[derive(Deserialize, Serialize, Debug)]
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
                if let Some(seconds) = noise.fade_duration_seconds {
                    ensure_seconds(seconds, context)?;
                }
            }
        }
        Ok(())
    }

    pub async fn segment(&self, playback: &PlaybackConfig, bot_state: &Data) -> Box<dyn Segment> {
        match self {
            PoolEntry::Track(track) => Box::new(
                TrackSegment::from_entry(track.clone(), playback, &bot_state.duration_cache).await,
            ),
            PoolEntry::Hex(message) => Box::new(HexSegment::from_entry(message.clone(), playback)),
            PoolEntry::Noise(noise) => Box::new(NoiseSegment::from_entry(noise, playback)),
        }
    }
}

#[field_trait_impl(Weighted)]
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct SignalProfileEntry {
    pub noise_profile: String,
    pub weight: u32,
    pub fade_duration_seconds: f32,
    pub min_duration_seconds: f32,
    pub max_duration_seconds: f32,
}

impl RegistryEntry for DJConfig {
    const KIND: &'static str = "DJ config";

    fn name(&self) -> &str {
        &self.name
    }

    /// Rejects values that would make the DJ panic when it picks them.
    fn validate(&self) -> anyhow::Result<()> {
        self.playback_config()
            .validate()
            .context("playback_config")?;
        for pool in self.pools.iter() {
            pool.validate(&self.segment_playback_config(Some(pool)))
                .with_context(|| format!("pool {:?}", pool.name))?;
        }
        let Some(signal_profiles) = &self.signal_profiles else {
            return Ok(());
        };
        ensure_penalty_multiplier(signal_profiles.history_penalty_multiplier)
            .context("signal_profiles")?;
        for profile in signal_profiles.entries.iter() {
            let context = || format!("signal profile {:?}", profile.noise_profile);
            ensure_seconds(profile.fade_duration_seconds, context)?;
            ensure_seconds(profile.min_duration_seconds, context)?;
            if profile.min_duration_seconds >= profile.max_duration_seconds {
                bail!(
                    "{}: min_duration_seconds must be less than max_duration_seconds",
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
