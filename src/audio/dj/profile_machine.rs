use crate::audio::dj::config::SignalProfileEntry;
use crate::audio::dj::segments::{
    CommonSegmentPlaybackConfig, Completed, SignalProfilePlayback, TimedSegment,
};
use crate::audio::dj::weighted_choice::WeightedSelector;
use rand::Rng;
use std::time::Duration;

pub struct ProfileStateMachine {
    /// `None` while a forced profile is active.
    current: Option<TimedSegment>,
    profiles: Vec<SignalProfileEntry>,
    selector: WeightedSelector,
    forced: SignalProfilePlayback,
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
    /// Falls back to a single `clear` profile when the list is empty.
    pub fn new(profiles: Vec<SignalProfileEntry>, initial_profile_name: Option<&str>) -> Self {
        let profiles = if profiles.is_empty() {
            vec![SignalProfileEntry {
                profile_name: "clear".to_string(),
                weight: 1,
                fade_duration_seconds: 3.0,
                min_time_seconds: 60.0,
                max_time_seconds: 120.0,
            }]
        } else {
            profiles
        };
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
            forced: SignalProfilePlayback::default(),
        }
    }

    /// Never resolves while a forced profile is active.
    pub fn ready_to_advance(&self) -> Completed {
        match &self.current {
            Some(segment) => segment.completed(),
            None => Completed::when(std::future::pending()),
        }
    }

    /// Follows the segment's forced profile and returns the transition
    /// (name, fade seconds, reason) it calls for, advancing normally while the
    /// forced profile is unchanged.
    pub fn update(&mut self, forced: SignalProfilePlayback) -> Option<(String, f32, &'static str)> {
        if forced.name == self.forced.name {
            if self.current.as_ref().is_some_and(TimedSegment::is_elapsed) {
                return self
                    .next_profile()
                    .map(|(profile_name, fade_secs)| (profile_name, fade_secs, ""));
            }
            return None;
        }
        let previous = std::mem::replace(&mut self.forced, forced);
        if let Some(profile) = self.forced.name.clone() {
            self.current = None;
            Some((profile, self.forced.fade_in().as_secs_f32(), "(forced)"))
        } else {
            self.next_profile().map(|(profile_name, _)| {
                (
                    profile_name,
                    previous.fade_out().as_secs_f32(),
                    "after releasing forced profile",
                )
            })
        }
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
