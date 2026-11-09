use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Announcement {
    pub message: String,
    pub discriminator: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct AnnouncementStateMachine {
    message: Option<String>,
    discriminator: String,
    last_applied: Option<Announcement>,
}

impl AnnouncementStateMachine {
    pub fn value(&self) -> Option<Announcement> {
        self.message.clone().map(|message| Announcement {
            message,
            discriminator: self.discriminator.clone(),
        })
    }

    pub fn start_segment(&mut self, message: Option<String>) {
        self.message = message;
        self.discriminator = format!("{:016x}", rand::random::<u64>());
    }

    pub fn changed(&mut self) -> Option<Announcement> {
        let current = self.value()?;
        if self.last_applied.as_ref() == Some(&current) {
            return None;
        }
        self.last_applied = Some(current.clone());
        Some(current)
    }
}
