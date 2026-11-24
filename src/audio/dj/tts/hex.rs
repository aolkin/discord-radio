use super::{EncodedMessage, EncodedStep, Encoder};
use rand::RngExt;
use std::time::Duration;

const DIGITS: &str = "0123456789ABCDEF";
const DIGIT_PAUSE: Duration = Duration::from_millis(50);
const BYTE_PAUSE: Duration = Duration::from_millis(800);

pub struct Hex;

impl Encoder for Hex {
    fn required_files(&self, prefix: &str) -> Vec<String> {
        DIGITS
            .chars()
            .map(|digit| file_name(prefix, digit))
            .collect()
    }

    fn encode(&self, prefix: &str, text: &str) -> anyhow::Result<EncodedMessage> {
        Ok(EncodedMessage {
            hint: mask_text(text),
            steps: steps(prefix, text),
        })
    }
}

fn file_name(prefix: &str, digit: char) -> String {
    format!("{prefix}hex_{digit}.wav")
}

fn steps(prefix: &str, text: &str) -> Vec<EncodedStep> {
    let sound = |nibble: u8| {
        EncodedStep::Sound(file_name(
            prefix,
            DIGITS.as_bytes()[usize::from(nibble)] as char,
        ))
    };
    text.bytes()
        .flat_map(|byte| {
            [
                sound(byte >> 4),
                EncodedStep::Pause(DIGIT_PAUSE),
                sound(byte & 0xF),
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
