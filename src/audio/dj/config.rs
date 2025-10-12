use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct DJConfig {
    pub name: String,
    pub track_pool: Vec<TrackEntry>,
    pub hex_messages: Vec<HexMessageEntry>,
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
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct HexMessageEntry {
    pub text: String,
    pub weight: u32,
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
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct StateWeights {
    pub track: u32,
    pub hex_message: u32,
    pub noise: u32,
    pub profile_change: u32,
}

impl DJConfig {
    pub fn load_from_file(path: &str) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let content = std::fs::read_to_string(path)?;
        let config: DJConfig = serde_json::from_str(&content)?;
        Ok(config)
    }
}
