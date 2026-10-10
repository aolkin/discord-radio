use crate::audio::dj::config::SignalProfileEntry;
use crate::audio::dj::segments::{
    CommonSegmentPlaybackConfig, SignalProfilePlayback, TimedSegment,
};
use crate::audio::dj::weighted_choice::WeightedSelector;
use rand::Rng;
use std::time::Duration;

pub struct ProfileStateMachine {
    /// `None` while a forced profile is active.
    current: Option<TimedSegment>,
    profiles: Vec<SignalProfileEntry>,
    selector: WeightedSelector,
}

fn profile_segment(entry: &SignalProfileEntry, hold_starts_after: Duration) -> TimedSegment {
    let duration_secs = rand::rng().random_range(entry.min_time_seconds..entry.max_time_seconds);
    let profile = SignalProfilePlayback {
        name: Some(entry.profile_name.clone()),
        fade_in_duration: Some(Duration::from_secs_f32(entry.fade_duration_seconds)),
        fade_out_duration: None,
    };
    let playback_config = CommonSegmentPlaybackConfig {
        signal_profile: Some(profile),
        channel_status: None,
    };
    TimedSegment::new(
        playback_config,
        hold_starts_after + Duration::from_secs_f32(duration_secs),
    )
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

        let current = Some(profile_segment(&profiles[initial_index], Duration::ZERO));

        let mut selector = WeightedSelector::new(5, 0.3);
        selector.add_to_history(&profiles[initial_index]);

        Self {
            current,
            profiles,
            selector,
        }
    }

    pub fn advance(&mut self) -> Option<(String, f32)> {
        if self.current.as_ref().is_some_and(TimedSegment::is_elapsed) {
            return self.next_profile();
        }
        None
    }

    pub fn force_profile(&mut self, _profile_name: String) {
        self.current = None;
    }

    pub fn release_forced_profile(&mut self) -> Option<(String, f32)> {
        if self.current.is_none() {
            // Always transition to next random profile
            return self.next_profile();
        }
        None
    }

    fn next_profile(&mut self) -> Option<(String, f32)> {
        let profile_entry = self.selector.choose(&self.profiles)?;
        let fade_duration_secs = profile_entry.fade_duration_seconds;
        let profile_name = profile_entry.profile_name.clone();

        self.current = Some(profile_segment(
            profile_entry,
            Duration::from_secs_f32(fade_duration_secs),
        ));

        Some((profile_name, fade_duration_secs))
    }
}
