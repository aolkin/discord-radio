use crate::audio::dj::weighted_choice::Weighted;
use crate::bucket::registry::RegistryEntry;
use anyhow::bail;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Clone, Debug, Default)]
pub struct DJConfig {
    pub name: String,
    #[serde(default)]
    pub track_pool: Vec<TrackEntry>,
    #[serde(default)]
    pub hex_messages: Vec<HexMessageEntry>,
    pub hex_message_announcements: Option<Vec<String>>,
    #[serde(default)]
    pub hex_message_defaults: HexMessageDefaults,
    pub noise_periods: Vec<NoisePeriodEntry>,
    pub signal_profiles: Vec<SignalProfileEntry>,
    pub state_weights: StateWeights,
    pub recent_history_size: usize,
    pub duplicate_penalty_multiplier: f32,
    pub channel_status: Option<String>,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct TrackEntry {
    pub filename: String,
    pub weight: u32,
    pub max_duration_seconds: Option<f32>,
    pub allow_subsection: Option<bool>,
    pub signal_profile: Option<String>,
    /// Volume level (0.0-2.0).
    ///
    /// Uses perceptual (logarithmic) scaling:
    /// - 0.0 = silence
    /// - 1.0 = unity gain (0 dB, original volume)
    /// - 2.0 = maximum boost (+18 dB, ~8x amplitude)
    ///
    /// This scaling provides more perceptually uniform volume changes
    /// compared to linear scaling.
    pub volume: Option<f32>,
    pub channel_status: Option<String>,
}

impl Weighted for TrackEntry {
    fn weight(&self) -> f32 {
        self.weight as f32
    }

    fn key(&self) -> &str {
        &self.filename
    }
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct HexMessageDefaults {
    #[serde(default = "default_loop_min")]
    pub loop_min: u32,
    #[serde(default = "default_loop_max")]
    pub loop_max: u32,
    #[serde(default)]
    pub signal_profile: Option<String>,
}

impl Default for HexMessageDefaults {
    fn default() -> Self {
        Self {
            loop_min: default_loop_min(),
            loop_max: default_loop_max(),
            signal_profile: None,
        }
    }
}

fn default_loop_min() -> u32 {
    1
}

fn default_loop_max() -> u32 {
    1
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct HexMessageEntry {
    pub text: String,
    pub weight: u32,
    pub signal_profile: Option<String>,
    pub loop_min: Option<u32>,
    pub loop_max: Option<u32>,
    pub announcement: Option<String>,
}

impl Weighted for HexMessageEntry {
    fn weight(&self) -> f32 {
        self.weight as f32
    }

    fn key(&self) -> &str {
        &self.text
    }
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct NoisePeriodEntry {
    pub noise_profile: String,
    pub min_duration_seconds: f32,
    pub max_duration_seconds: f32,
    pub weight: u32,
}

impl Weighted for NoisePeriodEntry {
    fn weight(&self) -> f32 {
        self.weight as f32
    }

    fn key(&self) -> &str {
        &self.noise_profile
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

    fn key(&self) -> &str {
        &self.profile_name
    }
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct StateWeights {
    pub track: u32,
    pub hex_message: u32,
    pub noise: u32,
}

impl Default for StateWeights {
    /// Equal non-zero weights: the scheduler skips categories whose pool is
    /// empty, but falls back to noise when every remaining weight is zero.
    fn default() -> Self {
        Self {
            track: 1,
            hex_message: 1,
            noise: 1,
        }
    }
}

impl RegistryEntry for DJConfig {
    const KIND: &'static str = "DJ config";

    fn name(&self) -> &str {
        &self.name
    }

    /// Rejects values that would make the DJ panic when it picks them.
    fn validate(&self) -> anyhow::Result<()> {
        for track in &self.track_pool {
            if let Some(seconds) = track.max_duration_seconds {
                ensure_seconds(seconds, || format!("track {:?}", track.filename))?;
            }
        }
        let defaults = &self.hex_message_defaults;
        ensure_loops(defaults.loop_min, defaults.loop_max, || {
            "hex_message_defaults".to_string()
        })?;
        for message in &self.hex_messages {
            ensure_loops(
                message.loop_min.unwrap_or(defaults.loop_min),
                message.loop_max.unwrap_or(defaults.loop_max),
                || format!("hex message {:?}", message.text),
            )?;
        }
        for noise in &self.noise_periods {
            let context = || format!("noise period {:?}", noise.noise_profile);
            ensure_seconds(noise.min_duration_seconds, context)?;
            ensure_seconds(noise.max_duration_seconds, context)?;
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

fn ensure_seconds(seconds: f32, context: impl Fn() -> String) -> anyhow::Result<()> {
    if !(seconds.is_finite() && seconds >= 0.0) {
        bail!(
            "{}: {seconds} is not a non-negative number of seconds",
            context()
        );
    }
    Ok(())
}

fn ensure_loops(min: u32, max: u32, context: impl Fn() -> String) -> anyhow::Result<()> {
    if min > max {
        bail!("{}: loop_min must not exceed loop_max", context());
    }
    Ok(())
}
