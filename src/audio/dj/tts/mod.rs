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

#[derive(Clone, Debug, Default)]
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
    fn sounds(&self) -> &'static [&'static str];
    fn file_name(&self, prefix: &str, sound: &str) -> String;
    fn encode(&self, text: &str) -> anyhow::Result<EncodedMessage>;
}

impl TtsEncoding {
    fn encoder(self) -> &'static dyn Encoder {
        match self {
            Self::Hex => &hex::Hex,
        }
    }

    pub fn file_name(self, prefix: &str, sound: &str) -> String {
        self.encoder().file_name(prefix, sound)
    }

    pub fn file_names(self, prefix: &str) -> Vec<String> {
        let encoder = self.encoder();
        encoder
            .sounds()
            .iter()
            .map(|sound| encoder.file_name(prefix, sound))
            .collect()
    }

    pub fn encode(self, text: &str) -> anyhow::Result<EncodedMessage> {
        self.encoder().encode(text)
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
        .file_names("")
        .into_iter()
        .filter(|name| !present.contains(name))
        .collect())
}
