use super::{EncodedMessage, EncodedStep, Encoder};
use anyhow::bail;
use rand::RngExt;
use std::time::Duration;

const UNIT: Duration = Duration::from_millis(100);
const SYMBOL_UNITS: u32 = 1;
const LETTER_UNITS: u32 = 3;
const WORD_UNITS: u32 = 7;

const CODES: [(char, &str); 36] = [
    ('A', ".-"),
    ('B', "-..."),
    ('C', "-.-."),
    ('D', "-.."),
    ('E', "."),
    ('F', "..-."),
    ('G', "--."),
    ('H', "...."),
    ('I', ".."),
    ('J', ".---"),
    ('K', "-.-"),
    ('L', ".-.."),
    ('M', "--"),
    ('N', "-."),
    ('O', "---"),
    ('P', ".--."),
    ('Q', "--.-"),
    ('R', ".-."),
    ('S', "..."),
    ('T', "-"),
    ('U', "..-"),
    ('V', "...-"),
    ('W', ".--"),
    ('X', "-..-"),
    ('Y', "-.--"),
    ('Z', "--.."),
    ('0', "-----"),
    ('1', ".----"),
    ('2', "..---"),
    ('3', "...--"),
    ('4', "....-"),
    ('5', "....."),
    ('6', "-...."),
    ('7', "--..."),
    ('8', "---.."),
    ('9', "----."),
];

pub struct Morse;

pub const MORSE: Morse = Morse;

fn pause(units: u32) -> EncodedStep {
    EncodedStep::Pause(UNIT * units)
}

fn code_for(c: char) -> Option<&'static str> {
    let upper = c.to_ascii_uppercase();
    CODES
        .iter()
        .find(|(letter, _)| *letter == upper)
        .map(|(_, code)| *code)
}

impl Encoder for Morse {
    fn required_files(&self, prefix: &str) -> Vec<String> {
        ["dot", "dash"]
            .into_iter()
            .map(|sound| format!("{prefix}morse_{sound}.wav"))
            .collect()
    }

    fn encode(&self, prefix: &str, text: &str) -> anyhow::Result<EncodedMessage> {
        let mut unplayable: Vec<char> = text
            .chars()
            .filter(|c| !c.is_whitespace() && code_for(*c).is_none())
            .collect();
        unplayable.sort();
        unplayable.dedup();
        if !unplayable.is_empty() {
            let names: Vec<String> = unplayable.iter().map(|c| format!("{c:?}")).collect();
            bail!(
                "morse can only play A-Z and 0-9; cannot play {}",
                names.join(", ")
            );
        }

        let mut rng = rand::rng();
        let mut hint = String::new();
        let mut steps = Vec::new();
        let mut letters = text.chars().peekable();
        while let Some(c) = letters.next() {
            let Some(code) = code_for(c) else {
                hint.push(c);
                continue;
            };
            hint.push(if rng.random_bool(0.5) { '·' } else { '−' });
            for symbol in code.chars() {
                let name = if symbol == '.' { "dot" } else { "dash" };
                steps.push(EncodedStep::Sound(format!("{prefix}morse_{name}.wav")));
                steps.push(pause(SYMBOL_UNITS));
            }
            steps.pop();
            steps.push(pause(
                if letters.peek().is_some_and(|next| code_for(*next).is_some()) {
                    LETTER_UNITS
                } else {
                    WORD_UNITS
                },
            ));
        }
        Ok(EncodedMessage { hint, steps })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary(message: &EncodedMessage) -> String {
        message
            .steps
            .iter()
            .map(|step| match step {
                EncodedStep::Sound(name) if name.ends_with("dot.wav") => ".".to_string(),
                EncodedStep::Sound(_) => "-".to_string(),
                EncodedStep::Pause(duration) => format!("[{}]", duration.as_millis()),
            })
            .collect()
    }

    #[test]
    fn spaces_letters_and_words() {
        let message = MORSE.encode("", "a 1").unwrap();
        assert_eq!(
            summary(&message),
            ".[100]-[700].[100]-[100]-[100]-[100]-[700]"
        );
        assert_eq!(message.hint.chars().count(), 3);
        assert_eq!(message.hint.chars().nth(1), Some(' '));
    }

    #[test]
    fn rejects_unplayable_characters() {
        let error = MORSE.encode("", "hi!? é!").unwrap_err().to_string();
        assert!(error.contains("'!'") && error.contains("'?'") && error.contains("'é'"));
    }
}
