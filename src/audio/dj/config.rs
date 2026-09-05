use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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
    /// Playlist name to R2 key, for track pools stored outside the config file.
    #[serde(default)]
    pub playlists: HashMap<String, String>,
    pub default_playlist: Option<String>,
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
    pub fn with_overrides(mut self, overrides: &crate::persistence::DJConfigOverrides) -> Self {
        if overrides.hex_messages.enabled && !overrides.hex_messages.items.is_empty() {
            self.hex_messages = overrides.hex_messages.items.clone();
        }

        if overrides.hex_message_announcements.enabled
            && !overrides.hex_message_announcements.items.is_empty()
        {
            self.hex_message_announcements =
                Some(overrides.hex_message_announcements.items.clone());
        }

        if overrides.state_weights.enabled
            && let Some(ref weights) = overrides.state_weights.value
        {
            self.state_weights = weights.clone();
        }

        self
    }

    /// Replaces `track_pool` with the contents of an external playlist file,
    /// falling back to the inline `track_pool` if no playlist is selected,
    /// the name isn't in `playlists`, or fetching/parsing it fails.
    pub async fn resolve_track_pool(
        &mut self,
        playlist_name: Option<&str>,
        file_cache: &crate::bucket::FileCache,
    ) {
        let Some(name) = playlist_name.or(self.default_playlist.as_deref()) else {
            return;
        };

        let Some(key) = self.playlists.get(name).cloned() else {
            tracing::warn!(
                "DJ config '{}' has no playlist named '{}', using inline track pool",
                self.name,
                name
            );
            return;
        };

        let tracks = match file_cache.ensure_cached(&key).await {
            Ok(path) => std::fs::read_to_string(&path)
                .map_err(|e| e.to_string())
                .and_then(|content| {
                    serde_json::from_str::<Vec<TrackEntry>>(&content).map_err(|e| e.to_string())
                }),
            Err(e) => Err(e.to_string()),
        };

        match tracks {
            Ok(tracks) => self.track_pool = tracks,
            Err(e) => tracing::warn!(
                "Failed to load playlist '{}' ({key}) for DJ config '{}': {e}, using inline track pool",
                name,
                self.name
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bucket::FileCache;
    use crate::bucket::cache::ObjectDownloader;
    use async_trait::async_trait;
    use std::error::Error;
    use std::sync::Arc;

    fn track_entry(filename: &str) -> TrackEntry {
        TrackEntry {
            filename: filename.to_string(),
            weight: 1,
            max_duration_seconds: None,
            allow_subsection: None,
            signal_profile: None,
            volume: None,
            channel_status: None,
        }
    }

    fn base_config() -> DJConfig {
        DJConfig {
            name: "test".to_string(),
            track_pool: vec![track_entry("inline.mp3")],
            hex_messages: vec![],
            hex_message_announcements: None,
            hex_message_defaults: HexMessageDefaults::default(),
            noise_periods: vec![],
            signal_profiles: vec![],
            state_weights: StateWeights {
                track: 1,
                hex_message: 1,
                noise: 1,
            },
            recent_history_size: 1,
            duplicate_penalty_multiplier: 1.0,
            channel_status: None,
            playlists: HashMap::new(),
            default_playlist: None,
        }
    }

    struct PlaylistDownloader(Vec<u8>);

    #[async_trait]
    impl ObjectDownloader for PlaylistDownloader {
        async fn download(&self, _key: &str) -> Result<Vec<u8>, Box<dyn Error + Send + Sync>> {
            Ok(self.0.clone())
        }
    }

    async fn cache_serving(tracks: &[TrackEntry]) -> FileCache {
        let dir = tempfile::tempdir().unwrap();
        FileCache::with_downloader(
            dir.path().to_path_buf(),
            Arc::new(PlaylistDownloader(serde_json::to_vec(tracks).unwrap())),
        )
        .await
    }

    async fn cache_with_no_downloader() -> FileCache {
        let dir = tempfile::tempdir().unwrap();
        FileCache::new(dir.path().to_path_buf(), None, None)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn resolve_track_pool_uses_an_explicit_playlist_when_found() {
        let cache = cache_serving(&[track_entry("remote.mp3")]).await;
        let mut config = base_config();
        config
            .playlists
            .insert("party".to_string(), "playlists/party.json".to_string());

        config.resolve_track_pool(Some("party"), &cache).await;

        assert_eq!(config.track_pool.len(), 1);
        assert_eq!(config.track_pool[0].filename, "remote.mp3");
    }

    #[tokio::test]
    async fn resolve_track_pool_falls_back_when_the_name_is_not_in_the_map() {
        let cache = cache_with_no_downloader().await;
        let mut config = base_config();

        config.resolve_track_pool(Some("missing"), &cache).await;

        assert_eq!(config.track_pool[0].filename, "inline.mp3");
    }

    #[tokio::test]
    async fn resolve_track_pool_falls_back_with_no_name_and_no_default() {
        let cache = cache_with_no_downloader().await;
        let mut config = base_config();

        config.resolve_track_pool(None, &cache).await;

        assert_eq!(config.track_pool[0].filename, "inline.mp3");
    }

    #[tokio::test]
    async fn resolve_track_pool_uses_the_default_playlist_when_present() {
        let cache = cache_serving(&[track_entry("remote.mp3")]).await;
        let mut config = base_config();
        config.default_playlist = Some("party".to_string());
        config
            .playlists
            .insert("party".to_string(), "playlists/party.json".to_string());

        config.resolve_track_pool(None, &cache).await;

        assert_eq!(config.track_pool[0].filename, "remote.mp3");
    }
}
