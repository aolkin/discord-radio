use anyhow::Context;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Clone, Debug)]
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

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct NoisePeriodEntry {
    pub noise_profile: String,
    pub min_duration_seconds: f32,
    pub max_duration_seconds: f32,
    pub weight: u32,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct SignalProfileEntry {
    pub profile_name: String,
    pub weight: u32,
    pub fade_duration_seconds: f32,
    pub min_time_seconds: f32,
    pub max_time_seconds: f32,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct StateWeights {
    pub track: u32,
    pub hex_message: u32,
    pub noise: u32,
}

impl DJConfig {
    pub fn load_from_file(path: &str) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let content = std::fs::read_to_string(path)?;
        let config: DJConfig = serde_json::from_str(&content)?;
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
            Some(uri) => load_pool(uri, resolver).await?,
            None => Vec::new(),
        };
        self.hex_messages = match &settings.hex_messages {
            Some(uri) => load_pool(uri, resolver).await?,
            None => Vec::new(),
        };
        Ok(())
    }
}

async fn load_pool<T: serde::de::DeserializeOwned>(
    uri: &str,
    resolver: &crate::bucket::FileResolver,
) -> anyhow::Result<Vec<T>> {
    let path = resolver
        .resolve(uri)
        .await
        .with_context(|| format!("resolving DJ component '{uri}'"))?;
    let contents = tokio::fs::read_to_string(&path)
        .await
        .with_context(|| format!("reading DJ component '{uri}'"))?;
    serde_json::from_str(&contents).with_context(|| format!("parsing DJ component '{uri}'"))
}
