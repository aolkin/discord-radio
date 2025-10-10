use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(rename_all = "lowercase")]
pub enum NoiseType {
    White,
    Pink,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct SignalProfile {
    pub name: String,

    pub bandpass_low: f32,
    pub bandpass_high: f32,

    pub noise_level: f32,
    pub noise_type: NoiseType,

    pub tremolo_depth: f32,
    pub tremolo_rate: f32,

    pub clip_threshold: f32,
    pub bitcrush_bits: Option<u8>,

    pub dropout_probability: f32,
    pub dropout_duration_ms: (f32, f32),

    pub pitch_shift_cents: Option<f32>,
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
            noise_level: self.noise_level * inv_t + other.noise_level * t,
            noise_type: if t < 0.5 {
                self.noise_type.clone()
            } else {
                other.noise_type.clone()
            },
            tremolo_depth: self.tremolo_depth * inv_t + other.tremolo_depth * t,
            tremolo_rate: self.tremolo_rate * inv_t + other.tremolo_rate * t,
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
            pitch_shift_cents: match (self.pitch_shift_cents, other.pitch_shift_cents) {
                (Some(a), Some(b)) => Some(a * inv_t + b * t),
                (Some(a), None) => Some(a * inv_t),
                (None, Some(b)) => Some(b * t),
                (None, None) => None,
            },
            frequency_warble_hz: match (self.frequency_warble_hz, other.frequency_warble_hz) {
                (Some(a), Some(b)) => Some(a * inv_t + b * t),
                (Some(a), None) => Some(a * inv_t),
                (None, Some(b)) => Some(b * t),
                (None, None) => None,
            },
        }
    }
}

pub struct ProfileManager {
    profiles: HashMap<String, SignalProfile>,
    profiles_dir: PathBuf,
}

impl ProfileManager {
    pub fn new(profiles_dir: impl Into<PathBuf>) -> Self {
        Self {
            profiles: HashMap::new(),
            profiles_dir: profiles_dir.into(),
        }
    }

    pub fn load_all(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if !self.profiles_dir.exists() {
            std::fs::create_dir_all(&self.profiles_dir)?;
            tracing::warn!(
                "Audio profiles directory created at: {}",
                self.profiles_dir.display()
            );
            return Ok(());
        }

        for entry in std::fs::read_dir(&self.profiles_dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.extension().and_then(|s| s.to_str()) == Some("json") {
                match self.load_profile(&path) {
                    Ok(profile) => {
                        tracing::info!("Loaded audio profile: {}", profile.name);
                        self.profiles.insert(profile.name.clone(), profile);
                    }
                    Err(e) => {
                        tracing::warn!("Failed to load profile from {}: {}", path.display(), e);
                    }
                }
            }
        }

        if self.profiles.is_empty() {
            tracing::warn!("No audio profiles loaded");
        }

        Ok(())
    }

    fn load_profile(
        &self,
        path: &Path,
    ) -> Result<SignalProfile, Box<dyn std::error::Error + Send + Sync>> {
        let content = std::fs::read_to_string(path)?;
        let profile: SignalProfile = serde_json::from_str(&content)?;
        Ok(profile)
    }

    pub fn get_profile(&self, name: &str) -> Option<&SignalProfile> {
        self.profiles.get(name)
    }

    pub fn list_profiles(&self) -> Vec<String> {
        self.profiles.keys().cloned().collect()
    }

    pub fn interpolate(&self, from_name: &str, to_name: &str, t: f32) -> Option<SignalProfile> {
        let from = self.profiles.get(from_name)?;
        let to = self.profiles.get(to_name)?;
        Some(from.interpolate(to, t))
    }
}
