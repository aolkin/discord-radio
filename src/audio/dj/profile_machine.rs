use crate::audio::dj::config::SignalProfileEntry;
use crate::audio::dj::weighted_choice::WeightedSelector;
use rand::Rng;
use std::ops::Add;
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
pub enum ProfileState {
    Active {
        started_at: Instant,
        duration: Duration,
    },
    ForcedProfile,
}

impl ProfileState {
    pub fn should_transition(&self) -> bool {
        match self {
            ProfileState::Active {
                started_at,
                duration,
                ..
            } => started_at.elapsed() >= *duration,
            ProfileState::ForcedProfile => false,
        }
    }
}

pub struct ProfileStateMachine {
    current_state: ProfileState,
    profiles: Vec<SignalProfileEntry>,
    selector: WeightedSelector,
}

impl ProfileStateMachine {
    pub fn new(profiles: Vec<SignalProfileEntry>, initial_profile_name: Option<&str>) -> Self {
        // Find matching profile by name, or default to first profile
        let initial_index = if let Some(name) = initial_profile_name {
            profiles
                .iter()
                .position(|p| p.profile_name == name)
                .unwrap_or(0)
        } else {
            0
        };

        let profile_entry = &profiles[initial_index];
        let mut rng = rand::rng();
        let duration_secs =
            rng.random_range(profile_entry.min_time_seconds..profile_entry.max_time_seconds);

        let mut selector = WeightedSelector::new(5, 0.3);
        selector.add_to_history(initial_index);

        Self {
            current_state: ProfileState::Active {
                started_at: Instant::now(),
                duration: Duration::from_secs_f32(duration_secs),
            },
            profiles,
            selector,
        }
    }

    pub fn advance(&mut self) -> Option<(String, f32)> {
        if self.current_state.should_transition() {
            return self.next_profile();
        }
        None
    }

    pub fn force_profile(&mut self, _profile_name: String) {
        self.current_state = ProfileState::ForcedProfile;
    }

    pub fn release_forced_profile(&mut self) -> Option<(String, f32)> {
        if let ProfileState::ForcedProfile = &self.current_state {
            // Always transition to next random profile
            return self.next_profile();
        }
        None
    }

    fn next_profile(&mut self) -> Option<(String, f32)> {
        let next_index = self.selector.choose(&self.profiles);
        let profile_entry = &self.profiles[next_index];

        let mut rng = rand::rng();
        let duration_secs =
            rng.random_range(profile_entry.min_time_seconds..profile_entry.max_time_seconds);

        let fade_duration_secs = profile_entry.fade_duration_seconds;
        let profile_name = profile_entry.profile_name.clone();

        // Immediately set to active state - fade happens in background
        self.current_state = ProfileState::Active {
            started_at: Instant::now().add(Duration::from_secs_f32(fade_duration_secs)),
            duration: Duration::from_secs_f32(duration_secs),
        };

        Some((profile_name, fade_duration_secs))
    }
}
