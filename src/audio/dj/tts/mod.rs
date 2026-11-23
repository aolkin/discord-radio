mod hex;

use crate::bucket::FileResolver;
use anyhow::Context;
use std::time::Duration;

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
            Self::Hex => hex::SOUNDS,
        }
    }

    pub fn file_name(self, sound: &str) -> String {
        match self {
            Self::Hex => hex::file_name(sound),
        }
    }

    pub fn encode(self, text: &str) -> anyhow::Result<EncodedMessage> {
        match self {
            Self::Hex => hex::encode(text),
        }
    }
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
