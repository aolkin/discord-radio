use super::{EncodedMessage, EncodedStep, Encoder};
use rand::RngExt;
use std::time::Duration;

const DIGIT_PAUSE: Duration = Duration::from_millis(50);
const BYTE_PAUSE: Duration = Duration::from_millis(800);

pub struct Digits {
    name: &'static str,
    digits: &'static str,
}

pub const HEX: Digits = Digits {
    name: "hex",
    digits: "0123456789ABCDEF",
};

pub const BASE36: Digits = Digits {
    name: "base36",
    digits: "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ",
};

impl Encoder for Digits {
    fn required_files(&self, prefix: &str) -> Vec<String> {
        self.digits
            .chars()
            .map(|digit| self.file_name(prefix, digit))
            .collect()
    }

    fn encode(&self, prefix: &str, text: &str) -> anyhow::Result<EncodedMessage> {
        Ok(EncodedMessage {
            hint: mask_text(text),
            steps: self.steps(prefix, text),
        })
    }
}

impl Digits {
    fn file_name(&self, prefix: &str, digit: char) -> String {
        format!("{prefix}{}_{digit}.wav", self.name)
    }

    fn steps(&self, prefix: &str, text: &str) -> Vec<EncodedStep> {
        let radix = self.digits.len() as u8;
        let sound = |digit: u8| {
            let digit = self.digits.as_bytes()[usize::from(digit)] as char;
            EncodedStep::Sound(self.file_name(prefix, digit))
        };
        text.bytes()
            .flat_map(|byte| {
                [
                    sound(byte / radix),
                    EncodedStep::Pause(DIGIT_PAUSE),
                    sound(byte % radix),
                    EncodedStep::Pause(BYTE_PAUSE),
                ]
            })
            .collect()
    }
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
