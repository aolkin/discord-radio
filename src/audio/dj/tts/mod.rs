mod hex;

use crate::bucket::FileResolver;
use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TtsEncoding {
    #[default]
    Hex,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EncodedStep {
    Sound(String),
    Pause(Duration),
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
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

trait Encoder {
    fn required_files(&self, prefix: &str) -> Vec<String>;
    fn encode(&self, prefix: &str, text: &str) -> anyhow::Result<EncodedMessage>;
}

impl TtsEncoding {
    fn encoder(self) -> &'static dyn Encoder {
        match self {
            Self::Hex => &hex::Hex,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Hex => "hex",
        }
    }

    pub fn required_files(self, prefix: &str) -> Vec<String> {
        self.encoder().required_files(prefix)
    }

    pub fn encode(self, prefix: &str, text: &str) -> anyhow::Result<EncodedMessage> {
        self.encoder().encode(prefix, text)
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
        .required_files("")
        .into_iter()
        .filter(|name| !present.contains(name))
        .collect())
}
