use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Announcement {
    pub message: String,
    pub discriminator: u64,
}

impl Announcement {
    pub fn new(message: String) -> Self {
        Self {
            message,
            discriminator: rand::random(),
        }
    }
}
