use crate::audio::dj::tiered::ConfigurableTier;
use serde::{Deserialize, Serialize};
use std::time::Duration;

pub const DEFAULT_PROFILE_FADE: Duration = Duration::from_secs(2);

pub fn default_profile_fade(tier: ConfigurableTier) -> Option<Duration> {
    match tier {
        ConfigurableTier::Manual => None,
        ConfigurableTier::Override => Some(DEFAULT_PROFILE_FADE),
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SignalProfilePlayback {
    pub name: String,
    pub fade_in_duration: Option<Duration>,
    pub fade_out_duration: Option<Duration>,
}

impl SignalProfilePlayback {
    pub fn fade_in(&self) -> Duration {
        self.fade_in_duration
            .unwrap_or_else(|| Duration::from_secs(1))
    }

    pub fn fade_out(&self) -> Duration {
        self.fade_out_duration
            .unwrap_or_else(|| Duration::from_secs_f32(1.5))
    }
}

impl From<String> for SignalProfilePlayback {
    fn from(name: String) -> Self {
        Self {
            name,
            fade_in_duration: None,
            fade_out_duration: None,
        }
    }
}

impl<T> From<&T> for SignalProfilePlayback
where
    T: Clone + Into<String>,
{
    fn from(value: &T) -> Self {
        let name: String = value.clone().into();
        name.into()
    }
}
