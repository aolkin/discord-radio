use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct DJConfig {
    pub name: String,
    pub track_pool: Vec<TrackEntry>,
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
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct NoisePeriodEntry {
    #[serde(rename = "type")]
    pub noise_type: NoiseTypeConfig,
    pub duration_range_seconds: (f32, f32),
    pub weight: u32,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(rename_all = "lowercase")]
pub enum NoiseTypeConfig {
    Static,
    Brown,
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
}
