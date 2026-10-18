use crate::audio::dj::weighted_choice::Weighted;
use anyhow::Context;
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

#[derive(Deserialize, Serialize, Clone, Debug, Default)]
pub struct HexComponent {
    #[serde(default)]
    pub defaults: HexMessageDefaults,
    #[serde(default)]
    pub announcements: Vec<String>,
    #[serde(default)]
    pub messages: Vec<HexMessageEntry>,
}

#[derive(Deserialize, Serialize, Clone, Debug, Default)]
pub struct ConfigComponent {
    #[serde(default)]
    pub noise_periods: Vec<NoisePeriodEntry>,
    #[serde(default)]
    pub state_weights: StateWeights,
    #[serde(default)]
    pub playback: PlaybackSettings,
    #[serde(default)]
    pub signal_profiles: Vec<SignalProfileEntry>,
    #[serde(default)]
    pub channel_status: Option<String>,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct PlaybackSettings {
    pub recent_history_size: usize,
    pub duplicate_penalty_multiplier: f32,
}

impl Default for PlaybackSettings {
    fn default() -> Self {
        Self {
            recent_history_size: 12,
            duplicate_penalty_multiplier: 0.5,
        }
    }
}

impl DJConfig {
    pub fn load_from_file(path: &str) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path).with_context(|| format!("reading {path}"))?;
        let config: DJConfig =
            serde_json::from_str(&content).with_context(|| format!("parsing {path}"))?;
        Ok(config)
    }

    /// Apply overrides to this config, replacing specified categories entirely when enabled
    pub fn with_overrides(mut self, settings: &crate::persistence::DjSettings) -> Self {
        if settings.hex_message_overrides.enabled
            && !settings.hex_message_overrides.items.is_empty()
        {
            self.hex_messages = settings.hex_message_overrides.items.clone();
        }

        if settings.hex_message_announcement_overrides.enabled
            && !settings.hex_message_announcement_overrides.items.is_empty()
        {
            self.hex_message_announcements =
                Some(settings.hex_message_announcement_overrides.items.clone());
        }

        if settings.state_weight_overrides.enabled
            && let Some(ref weights) = settings.state_weight_overrides.value
        {
            self.state_weights = weights.clone();
        }

        self
    }

    pub async fn apply_dj_settings(
        &mut self,
        settings: &crate::persistence::DjSettings,
        resolver: &crate::bucket::FileResolver,
    ) -> anyhow::Result<()> {
        self.track_pool = match &settings.tracks {
            Some(uri) => load_component::<Vec<TrackEntry>>(uri, resolver).await?,
            None => Vec::new(),
        };

        let hex = match &settings.hex_messages {
            Some(uri) => load_component::<HexComponent>(uri, resolver).await?,
            None => HexComponent::default(),
        };
        self.hex_messages = hex.messages;
        self.hex_message_defaults = hex.defaults;
        self.hex_message_announcements = Some(hex.announcements);

        let orchestration = match &settings.config {
            Some(uri) => load_component::<ConfigComponent>(uri, resolver).await?,
            None => ConfigComponent::default(),
        };
        self.noise_periods = orchestration.noise_periods;
        self.state_weights = orchestration.state_weights;
        self.recent_history_size = orchestration.playback.recent_history_size;
        self.duplicate_penalty_multiplier = orchestration.playback.duplicate_penalty_multiplier;
        self.signal_profiles = orchestration.signal_profiles;
        self.channel_status = orchestration.channel_status;

        Ok(())
    }
}

async fn load_component<T: serde::de::DeserializeOwned>(
    uri: &str,
    resolver: &crate::bucket::FileResolver,
) -> anyhow::Result<T> {
    let bytes = resolver
        .resolve_contents(uri)
        .await
        .with_context(|| format!("resolving DJ component '{uri}'"))?;
    serde_json::from_slice(&bytes).with_context(|| format!("parsing DJ component '{uri}'"))
}

#[cfg(test)]
mod tests;
