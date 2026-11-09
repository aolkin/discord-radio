use crate::audio::dj::tiered::TimedValue;
use crate::audio::dj::weighted_choice::{Identity, Weighted};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Announcement {
    pub message: String,
    pub discriminator: String,
}

impl Announcement {
    pub fn new(message: String) -> Self {
        Self {
            message,
            discriminator: format!("{:016x}", rand::random::<u64>()),
        }
    }
}

#[derive(Default)]
pub struct NoAnnouncement;

impl Weighted for NoAnnouncement {
    fn weight(&self) -> u32 {
        0
    }

    fn identity(&self) -> Identity {
        Identity::of(())
    }
}

impl From<&NoAnnouncement> for TimedValue<Option<Announcement>> {
    fn from(_: &NoAnnouncement) -> Self {
        TimedValue::permanent(None)
    }
}
