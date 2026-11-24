use super::{EncodedMessage, EncodedStep, Encoder};
use rand::RngExt;
use std::time::Duration;

const SOUNDS: &[&str] = &[
    "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "A", "B", "C", "D", "E", "F",
];
const DIGIT_PAUSE: Duration = Duration::from_millis(50);
const BYTE_PAUSE: Duration = Duration::from_millis(800);

pub struct Hex;

impl Encoder for Hex {
    fn sounds(&self) -> &'static [&'static str] {
        SOUNDS
    }

    fn file_name(&self, prefix: &str, sound: &str) -> String {
        format!("{prefix}hex_{sound}.wav")
    }

    fn encode(&self, text: &str) -> anyhow::Result<EncodedMessage> {
        Ok(EncodedMessage {
            hint: mask_text(text),
            steps: steps(text),
        })
    }
}

fn steps(text: &str) -> Vec<EncodedStep> {
    text.bytes()
        .flat_map(|byte| {
            [
                EncodedStep::Sound(SOUNDS[usize::from(byte >> 4)]),
                EncodedStep::Pause(DIGIT_PAUSE),
                EncodedStep::Sound(SOUNDS[usize::from(byte & 0xF)]),
                EncodedStep::Pause(BYTE_PAUSE),
            ]
        })
        .collect()
}

fn mask_text(text: &str) -> String {
    let mut rng = rand::rng();
    text.chars()
        .map(|c| {
            if c.is_alphanumeric() {
                if rng.random_bool(0.5) { '?' } else { '¿' }
            } else {
                c
            }
        })
        .collect()
}
