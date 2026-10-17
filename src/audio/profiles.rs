use crate::bucket::{ObjectStore, ObjectStoreError};
use anyhow::Context;
use futures_util::{StreamExt, stream};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, RwLock};

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct SignalProfile {
    pub name: String,

    pub bandpass_low: f32,
    pub bandpass_high: f32,

    /// Noise levels (0.0-2.0) use perceptual (dB-based) scaling:
    /// - 0.0 = silence
    /// - 0.5 = moderate (-30dB)
    /// - 1.0 = reference level (0dB)
    /// - 2.0 = boosted (+18dB)
    pub white_noise_level: f32,
    pub pink_noise_level: f32,
    pub brown_noise_level: f32,

    pub tremolo_depth: f32,
    pub tremolo_rate: f32,
    pub tremolo_jitter: f32,

    pub clip_pregain: f32,
    pub clip_threshold: f32,
    pub bitcrush_bits: Option<u8>,

    pub dropout_probability: f32,
    pub dropout_duration_ms: (f32, f32),

    pub frequency_warble_hz: Option<f32>,
}

impl SignalProfile {
    pub fn interpolate(&self, other: &Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        let inv_t = 1.0 - t;

        Self {
            name: format!("{}→{}", self.name, other.name),
            bandpass_low: self.bandpass_low * inv_t + other.bandpass_low * t,
            bandpass_high: self.bandpass_high * inv_t + other.bandpass_high * t,
            // Interpolate noise levels linearly in perceptual space.
            // The perceptual scale (0.0-2.0) already maps linearly to dB,
            // so linear interpolation here gives smooth perceptual transitions.
            white_noise_level: self.white_noise_level * inv_t + other.white_noise_level * t,
            pink_noise_level: self.pink_noise_level * inv_t + other.pink_noise_level * t,
            brown_noise_level: self.brown_noise_level * inv_t + other.brown_noise_level * t,
            tremolo_depth: self.tremolo_depth * inv_t + other.tremolo_depth * t,
            tremolo_rate: self.tremolo_rate * inv_t + other.tremolo_rate * t,
            tremolo_jitter: self.tremolo_jitter * inv_t + other.tremolo_jitter * t,
            clip_pregain: self.clip_pregain * inv_t + other.clip_pregain * t,
            clip_threshold: self.clip_threshold * inv_t + other.clip_threshold * t,
            bitcrush_bits: if t < 0.5 {
                self.bitcrush_bits
            } else {
                other.bitcrush_bits
            },
            dropout_probability: self.dropout_probability * inv_t + other.dropout_probability * t,
            dropout_duration_ms: (
                self.dropout_duration_ms.0 * inv_t + other.dropout_duration_ms.0 * t,
                self.dropout_duration_ms.1 * inv_t + other.dropout_duration_ms.1 * t,
            ),
            frequency_warble_hz: match (self.frequency_warble_hz, other.frequency_warble_hz) {
                (Some(a), Some(b)) => Some(a * inv_t + b * t),
                (Some(a), None) => Some(a * inv_t),
                (None, Some(b)) => Some(b * t),
                (None, None) => None,
            },
        }
    }
}

const REMOTE_PREFIX: &str = "profiles/";

#[derive(Debug, thiserror::Error)]
pub enum ProfileStorageError {
    #[error("profile names may only contain letters, digits, '-' and '_'")]
    InvalidName,
    #[error(transparent)]
    Storage(#[from] ObjectStoreError),
    #[error("parsing JSON: {0}")]
    Parse(#[from] serde_json::Error),
}

pub struct ProfileManager {
    profiles: RwLock<HashMap<String, SignalProfile>>,
    store: Arc<ObjectStore>,
}

impl ProfileManager {
    /// Loads every profile in `profiles_dir` and under `profiles/` in object
    /// storage. A profile in object storage replaces a local profile of the
    /// same name.
    pub async fn from_storage(profiles_dir: &Path, store: Arc<ObjectStore>) -> Self {
        let mut profiles = HashMap::new();

        let local_paths = match std::fs::read_dir(profiles_dir) {
            Ok(entries) => entries
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .collect(),
            Err(e) => {
                tracing::warn!(
                    "Failed to read signal profiles from {}: {e}",
                    profiles_dir.display()
                );
                Vec::new()
            }
        };
        let local = local_paths
            .into_iter()
            .filter(|path| path.extension().and_then(|s| s.to_str()) == Some("json"))
            .map(|path| {
                let bytes = std::fs::read(&path).context("reading file");
                (path.display().to_string(), bytes)
            });

        let remote: Vec<_> = stream::iter(store.list_remote(REMOTE_PREFIX).await)
            .filter(|key| std::future::ready(key.ends_with(".json")))
            .then(|key| async {
                let bytes = store.fetch_bytes(&key).await.context("fetching");
                (key, bytes)
            })
            .collect()
            .await;

        for (source, bytes) in local.chain(remote) {
            let parsed = bytes.and_then(|bytes| {
                serde_json::from_slice::<SignalProfile>(&bytes).context("parsing JSON")
            });
            match parsed {
                Ok(profile) => {
                    tracing::info!("Loaded signal profile {} from {source}", profile.name);
                    profiles.insert(profile.name.clone(), profile);
                }
                Err(e) => tracing::warn!("Failed to load signal profile from {source}: {e:#}"),
            }
        }

        if profiles.is_empty() {
            tracing::warn!("No signal profiles loaded");
        } else {
            tracing::info!("Loaded {} signal profiles", profiles.len());
        }

        Self {
            profiles: RwLock::new(profiles),
            store,
        }
    }

    pub fn get_profile(&self, name: &str) -> Option<SignalProfile> {
        self.profiles.read().unwrap().get(name).cloned()
    }

    pub fn list_profiles(&self) -> Vec<String> {
        self.profiles.read().unwrap().keys().cloned().collect()
    }

    /// The "clear" profile, or a built-in equivalent when none is loaded.
    pub fn default_profile(&self) -> SignalProfile {
        if let Some(profile) = self.get_profile("clear") {
            return profile;
        }

        tracing::debug!("Default 'clear' profile not found, using fallback");
        SignalProfile {
            name: "clear".to_string(),
            bandpass_low: 20.0,
            bandpass_high: 20000.0,
            white_noise_level: 0.0,
            pink_noise_level: 0.0,
            brown_noise_level: 0.0,
            tremolo_depth: 0.0,
            tremolo_rate: 0.0,
            tremolo_jitter: 0.0,
            clip_pregain: 1.0,
            clip_threshold: 1.0,
            bitcrush_bits: None,
            dropout_probability: 0.0,
            dropout_duration_ms: (0.0, 0.0),
            frequency_warble_hz: None,
        }
    }

    /// Writes `profile` to object storage under its name, replacing any
    /// profile of the same name.
    pub async fn save(&self, profile: SignalProfile) -> Result<(), ProfileStorageError> {
        let key = remote_key(&profile.name)?;
        let bytes = serde_json::to_vec_pretty(&profile).expect("a SignalProfile always serializes");
        self.store.upload(&key, &bytes, "application/json").await?;

        self.profiles
            .write()
            .unwrap()
            .insert(profile.name.clone(), profile);
        Ok(())
    }

    /// Re-fetches the profile named `name` from object storage and replaces
    /// the in-memory copy with it.
    pub async fn refresh(&self, name: &str) -> Result<SignalProfile, ProfileStorageError> {
        let bytes = self.store.fetch_bytes(&remote_key(name)?).await?;
        let profile: SignalProfile = serde_json::from_slice(&bytes)?;
        self.profiles
            .write()
            .unwrap()
            .insert(profile.name.clone(), profile.clone());
        Ok(profile)
    }
}

pub fn is_valid_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn remote_key(name: &str) -> Result<String, ProfileStorageError> {
    if !is_valid_name(name) {
        return Err(ProfileStorageError::InvalidName);
    }
    Ok(format!("{REMOTE_PREFIX}{name}.json"))
}
