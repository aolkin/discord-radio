use crate::audio::dj::segments::{
    CommonSegmentPlaybackConfig, HexMessageDefaults, HexMessageEntry, HexSegment, NoiseEntry,
    NoiseSegment, Segment, SignalProfilePlayback, TrackEntry, TrackSegment, ensure_loops,
};
use crate::audio::dj::state_machine::{ChannelStatus, DEFAULT_PROFILE_FADE};
use crate::audio::dj::tiered::TimedValue;
use crate::audio::dj::weighted_choice::{
    Identity, Weighted, WeightedPool, ambassador_impl_Weighted,
};
use crate::bucket::registry::RegistryEntry;
use crate::state::Data;
use ambassador::Delegate;
use anyhow::{Context, bail};
use field_trait::field_trait_impl;
use rand::RngExt;
use rand::seq::IndexedRandom;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::num::NonZeroU32;
use std::sync::{Arc, LazyLock};
use std::time::Duration;

pub mod registry;

#[derive(Deserialize, Serialize, Debug, Default)]
pub struct DJConfig {
    pub name: String,
    pub pools: BTreeMap<String, PoolConfig>,
    pub signal_profiles: SignalProfiles,
    pub channel_statuses: ChannelStatuses,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pool_history_penalty_multiplier: Option<f32>,
}

#[derive(Deserialize, Serialize, Debug)]
pub struct PoolConfig {
    pub weight: NonZeroU32,
    #[serde(default)]
    pub segment_defaults: SegmentDefaults,
}

#[derive(Debug, Clone)]
pub struct ConfiguredPool {
    pub weight: u32,
    pub segment_defaults: SegmentDefaults,
    pub file: Arc<Pool>,
}

pub struct LoadedConfig {
    pub config: Arc<DJConfig>,
    pub pools: Arc<[ConfiguredPool]>,
}

#[field_trait_impl(WeightedPool<SignalProfileEntry>)]
#[derive(Deserialize, Serialize, Debug, Default)]
pub struct SignalProfiles {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub history_penalty_multiplier: Option<f32>,
    pub entries: Arc<[SignalProfileEntry]>,
}

#[field_trait_impl(WeightedPool<ChannelStatusEntry>)]
#[derive(Deserialize, Serialize, Debug, Default)]
pub struct ChannelStatuses {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub history_penalty_multiplier: Option<f32>,
    pub entries: Arc<[ChannelStatusEntry]>,
}

#[field_trait_impl(WeightedPool<PoolEntry>)]
#[derive(Deserialize, Serialize, Debug)]
pub struct Pool {
    pub name: String,
    #[serde(default)]
    pub segment_defaults: SegmentDefaults,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub history_penalty_multiplier: Option<f32>,
    pub entries: Arc<[PoolEntry]>,
}

#[derive(Deserialize, Serialize, Clone, Debug, Default)]
pub struct SegmentDefaults {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hex_config: Option<HexMessageDefaults>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signal_profile: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel_status: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub announcements: Vec<String>,
}

static BUILT_IN_HEX_DEFAULTS: LazyLock<HexMessageDefaults> = LazyLock::new(Default::default);

impl SegmentDefaults {
    pub fn hex_config(&self) -> &HexMessageDefaults {
        self.hex_config.as_ref().unwrap_or(&BUILT_IN_HEX_DEFAULTS)
    }

    pub fn pick_announcement(&self, own: Option<String>) -> Option<String> {
        own.or_else(|| self.announcements.choose(&mut rand::rng()).cloned())
    }

    pub fn playback_config(&self, entry: EntryPlaybackConfig) -> CommonSegmentPlaybackConfig {
        CommonSegmentPlaybackConfig {
            signal_profile: entry
                .signal_profile
                .or_else(|| self.signal_profile.clone())
                .map(Into::into),
            channel_status: entry.channel_status.or_else(|| self.channel_status.clone()),
            announcement: self.pick_announcement(entry.announcement),
        }
    }

    pub fn or(&self, fallback: &Self) -> Self {
        Self {
            hex_config: self
                .hex_config
                .clone()
                .or_else(|| fallback.hex_config.clone()),
            signal_profile: self
                .signal_profile
                .clone()
                .or_else(|| fallback.signal_profile.clone()),
            channel_status: self
                .channel_status
                .clone()
                .or_else(|| fallback.channel_status.clone()),
            announcements: if self.announcements.is_empty() {
                fallback.announcements.clone()
            } else {
                self.announcements.clone()
            },
        }
    }
}

#[field_trait_impl]
impl Weighted for ConfiguredPool {
    fn identity(&self) -> Identity {
        Identity::of(&self.file.name)
    }
}

impl Pool {
    pub fn has_weighted_entry(&self) -> bool {
        self.entries.iter().any(|entry| entry.weight() > 0)
    }
}

impl RegistryEntry for Pool {
    const KIND: &'static str = "pool";

    fn name(&self) -> &str {
        &self.name
    }

    fn validate(&self) -> anyhow::Result<()> {
        let defaults = self.segment_defaults.hex_config();
        ensure_loops(defaults.loop_min, defaults.loop_max).context("hex_config")?;
        ensure_penalty_multiplier(self.history_penalty_multiplier)?;
        if !self.has_weighted_entry() {
            bail!("pool has no entry with weight above 0");
        }
        self.entries
            .iter()
            .try_for_each(|entry| entry.validate(defaults))
    }
}

/// The kind is inferred from which fields are present.
#[derive(Deserialize, Serialize, Debug, Delegate)]
#[serde(untagged)]
#[delegate(Weighted)]
pub enum PoolEntry {
    Track(TrackEntry),
    Hex(HexMessageEntry),
    Noise(NoiseEntry),
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
                let context = || format!("noise period {:?}", noise.playback.signal_profile);
                noise.duration.validate(context)?;
                if let Some(seconds) = noise.fade_duration_seconds {
                    ensure_seconds(seconds, context)?;
                }
            }
        }
        Ok(())
    }

    pub async fn segment(
        &self,
        segment_defaults: &SegmentDefaults,
        bot_state: &Data,
    ) -> Box<dyn Segment> {
        match self {
            PoolEntry::Track(track) => Box::new(
                TrackSegment::from_entry(
                    track.clone(),
                    segment_defaults,
                    &bot_state.duration_cache,
                )
                .await,
            ),
            PoolEntry::Hex(message) => {
                Box::new(HexSegment::from_entry(message.clone(), segment_defaults))
            }
            PoolEntry::Noise(noise) => Box::new(NoiseSegment::from_entry(noise, segment_defaults)),
        }
    }
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct ChannelStatusEntry {
    pub status: String,
    pub weight: u32,
    #[serde(flatten)]
    pub duration: DurationRange,
}

#[field_trait_impl]
impl Weighted for ChannelStatusEntry {
    fn identity(&self) -> Identity {
        Identity::of((
            &self.status,
            self.duration.min_duration_seconds.to_bits(),
            self.duration.max_duration_seconds.to_bits(),
        ))
    }
}

impl Default for ChannelStatusEntry {
    fn default() -> Self {
        Self {
            status: String::new(),
            weight: 1,
            duration: DurationRange::default(),
        }
    }
}

impl From<&ChannelStatusEntry> for TimedValue<ChannelStatus> {
    fn from(entry: &ChannelStatusEntry) -> Self {
        TimedValue::temporary(
            ChannelStatus::Fixed(entry.status.clone()),
            entry.duration.sample(),
        )
    }
}

impl RegistryEntry for DJConfig {
    const KIND: &'static str = "DJ config";

    fn name(&self) -> &str {
        &self.name
    }

    /// Rejects values that would make the DJ panic when it picks them.
    fn validate(&self) -> anyhow::Result<()> {
        ensure_penalty_multiplier(self.pool_history_penalty_multiplier)?;
        ensure_penalty_multiplier(self.signal_profiles.history_penalty_multiplier)
            .context("signal_profiles")?;
        for profile in self.signal_profiles.entries.iter() {
            let context = || format!("signal profile {:?}", profile.signal_profile);
            if let Some(seconds) = profile.fade_duration_seconds {
                ensure_seconds(seconds, context)?;
            }
            profile.duration.validate(context)?;
        }
        ensure_penalty_multiplier(self.channel_statuses.history_penalty_multiplier)
            .context("channel_statuses")?;
        for status in self.channel_statuses.entries.iter() {
            status
                .duration
                .validate(|| format!("channel status {:?}", status.status))?;
        }
        Ok(())
    }
}

#[derive(Deserialize, Serialize, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct EntryPlaybackConfig {
    pub signal_profile: Option<String>,
    pub channel_status: Option<String>,
    pub announcement: Option<String>,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct SignalProfileEntry {
    pub signal_profile: String,
    #[serde(flatten)]
    pub duration: DurationRange,
    pub weight: u32,
    pub fade_duration_seconds: Option<f32>,
}

#[field_trait_impl]
impl Weighted for SignalProfileEntry {
    fn identity(&self) -> Identity {
        Identity::of((
            &self.signal_profile,
            self.duration.min_duration_seconds.to_bits(),
            self.duration.max_duration_seconds.to_bits(),
            self.fade_duration_seconds.map(f32::to_bits),
        ))
    }
}

impl Default for SignalProfileEntry {
    fn default() -> Self {
        Self {
            signal_profile: "clear".to_string(),
            weight: 1,
            fade_duration_seconds: Some(3.0),
            duration: DurationRange::default(),
        }
    }
}

impl From<&SignalProfileEntry> for TimedValue<SignalProfilePlayback> {
    fn from(entry: &SignalProfileEntry) -> Self {
        let fade = entry
            .fade_duration_seconds
            .map_or(DEFAULT_PROFILE_FADE, Duration::from_secs_f32);
        let profile = SignalProfilePlayback {
            fade_in_duration: Some(fade),
            ..entry.signal_profile.clone().into()
        };
        TimedValue::temporary(profile, fade + entry.duration.sample())
    }
}

#[derive(Deserialize, Serialize, Clone, Copy, Debug)]
pub struct DurationRange {
    pub min_duration_seconds: f32,
    pub max_duration_seconds: f32,
}

impl DurationRange {
    pub fn sample(&self) -> Duration {
        let seconds = if self.min_duration_seconds >= self.max_duration_seconds {
            self.min_duration_seconds
        } else {
            rand::rng().random_range(self.min_duration_seconds..self.max_duration_seconds)
        };
        Duration::from_secs_f32(seconds)
    }

    pub fn validate(&self, context: impl Fn() -> String) -> anyhow::Result<()> {
        ensure_seconds(self.min_duration_seconds, &context)?;
        if self.min_duration_seconds >= self.max_duration_seconds {
            bail!(
                "{}: min_duration_seconds must be less than max_duration_seconds",
                context()
            );
        }
        Ok(())
    }
}

impl Default for DurationRange {
    fn default() -> Self {
        Self {
            min_duration_seconds: 60.0,
            max_duration_seconds: 120.0,
        }
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

pub fn optional_seconds(seconds: Option<f32>, name: &str) -> anyhow::Result<Option<Duration>> {
    seconds
        .map(|seconds| {
            ensure_seconds(seconds, || name.to_string())?;
            Ok(Duration::from_secs_f32(seconds))
        })
        .transpose()
}

impl WeightedPool<ConfiguredPool> for LoadedConfig {
    fn entries(&self) -> &Arc<[ConfiguredPool]> {
        &self.pools
    }

    fn history_penalty_multiplier(&self) -> Option<f32> {
        self.config.pool_history_penalty_multiplier
    }
}
