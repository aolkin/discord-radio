use crate::bucket::FileResolver;
use anyhow::Context;
use rand::RngExt;
use std::time::Duration;

const HEX_SOUNDS: &[&str] = &[
    "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "A", "B", "C", "D", "E", "F",
];
const HEX_DIGIT_PAUSE: Duration = Duration::from_millis(50);
const HEX_BYTE_PAUSE: Duration = Duration::from_millis(800);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TtsEncoding {
    #[default]
    Hex,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EncodedStep {
    Sound(&'static str),
    Pause(Duration),
}

#[derive(Clone, Debug)]
pub struct EncodedMessage {
    pub hint: String,
    pub steps: Vec<EncodedStep>,
}

impl EncodedMessage {
    pub fn sound_count(&self) -> usize {
        self.steps
            .iter()
            .filter(|step| matches!(step, EncodedStep::Sound(_)))
            .count()
    }
}

impl TtsEncoding {
    pub fn sounds(self) -> &'static [&'static str] {
        match self {
            Self::Hex => HEX_SOUNDS,
        }
    }

    pub fn file_name(self, sound: &str) -> String {
        match self {
            Self::Hex => format!("hex_{sound}.wav"),
        }
    }

    pub fn encode(self, text: &str) -> anyhow::Result<EncodedMessage> {
        match self {
            Self::Hex => Ok(EncodedMessage {
                hint: mask_text(text),
                steps: hex_steps(text),
            }),
        }
    }
}

fn hex_steps(text: &str) -> Vec<EncodedStep> {
    text.bytes()
        .flat_map(|byte| {
            [
                EncodedStep::Sound(HEX_SOUNDS[usize::from(byte >> 4)]),
                EncodedStep::Pause(HEX_DIGIT_PAUSE),
                EncodedStep::Sound(HEX_SOUNDS[usize::from(byte & 0xF)]),
                EncodedStep::Pause(HEX_BYTE_PAUSE),
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

pub async fn list_missing_files(
    file_resolver: &FileResolver,
    encoding: TtsEncoding,
    folder: &str,
) -> anyhow::Result<Vec<String>> {
    let present = file_resolver
        .list_files(folder)
        .await
        .with_context(|| format!("listing audio folder {folder:?}"))?;
    Ok(encoding
        .sounds()
        .iter()
        .map(|sound| encoding.file_name(sound))
        .filter(|name| !present.contains(name))
        .collect())
}
