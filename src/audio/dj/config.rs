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
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct PlaybackSettings {
    pub recent_history_size: usize,
    pub duplicate_penalty_multiplier: f32,
}

impl Default for PlaybackSettings {
    fn default() -> Self {
        Self {
            recent_history_size: 0,
            // `WeightedSelector` applies no repetition penalty at 0.0.
            duplicate_penalty_multiplier: 0.0,
        }
    }
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
mod tests {
    use super::*;
    use crate::bucket::{FileCache, FileResolver};
    use crate::persistence::DjSettings;
    use std::sync::Arc;

    fn local_file_config() -> DJConfig {
        DJConfig {
            name: "test".to_string(),
            track_pool: Vec::new(),
            hex_messages: vec![HexMessageEntry {
                text: "from the local file".to_string(),
                weight: 1,
                signal_profile: None,
                loop_min: None,
                loop_max: None,
                announcement: None,
            }],
            hex_message_announcements: Some(vec!["from the local file".to_string()]),
            hex_message_defaults: HexMessageDefaults {
                loop_min: 9,
                loop_max: 9,
                signal_profile: Some("local".to_string()),
            },
            noise_periods: vec![NoisePeriodEntry {
                noise_profile: "local".to_string(),
                min_duration_seconds: 1.0,
                max_duration_seconds: 2.0,
                weight: 1,
            }],
            signal_profiles: Vec::new(),
            state_weights: StateWeights {
                track: 7,
                hex_message: 7,
                noise: 7,
            },
            recent_history_size: 12,
            duplicate_penalty_multiplier: 0.5,
            channel_status: None,
        }
    }

    async fn resolver(content_path: &std::path::Path) -> FileResolver {
        let file_cache = Arc::new(
            FileCache::new(content_path.to_path_buf(), None, None)
                .await
                .unwrap(),
        );
        FileResolver::new(content_path.to_string_lossy().to_string(), file_cache)
    }

    #[tokio::test]
    async fn slots_replace_the_local_file_values_whether_set_or_cleared() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("hex.json"),
            r#"{"defaults":{"loop_min":4,"loop_max":6},"announcements":["slot"],
                "messages":[{"text":"slot","weight":2,"signal_profile":null,
                "loop_min":null,"loop_max":null,"announcement":null}]}"#,
        )
        .unwrap();
        std::fs::write(
            dir.path().join("config.json"),
            r#"{"state_weights":{"track":3,"hex_message":0,"noise":0},
                "playback":{"recent_history_size":2,"duplicate_penalty_multiplier":0.25}}"#,
        )
        .unwrap();
        let resolver = resolver(dir.path()).await;

        let mut config = local_file_config();
        config
            .apply_dj_settings(
                &DjSettings {
                    hex_messages: Some("hex.json".to_string()),
                    config: Some("config.json".to_string()),
                    ..Default::default()
                },
                &resolver,
            )
            .await
            .unwrap();

        assert_eq!(config.hex_messages.len(), 1);
        assert_eq!(config.hex_messages[0].text, "slot");
        assert_eq!(
            config.hex_message_announcements,
            Some(vec!["slot".to_string()])
        );
        assert_eq!(config.hex_message_defaults.loop_max, 6);
        assert!(config.noise_periods.is_empty());
        assert_eq!(config.state_weights.track, 3);
        assert_eq!(config.recent_history_size, 2);
        assert_eq!(config.duplicate_penalty_multiplier, 0.25);

        let mut config = local_file_config();
        config
            .apply_dj_settings(&DjSettings::default(), &resolver)
            .await
            .unwrap();

        assert!(config.hex_messages.is_empty());
        assert_eq!(config.hex_message_announcements, Some(Vec::new()));
        assert_eq!(config.hex_message_defaults.loop_max, default_loop_max());
        assert!(config.noise_periods.is_empty());
        assert_eq!(config.state_weights.track, StateWeights::default().track);
        assert_eq!(
            config.recent_history_size,
            PlaybackSettings::default().recent_history_size
        );
    }
}
