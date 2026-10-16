use crate::bucket::FileResolver;
use anyhow::{Context, bail};
use base64::Engine;
use base64::prelude::BASE64_STANDARD;
use serde::Deserialize;
use serde_json::{Map, Value};

/// Fields left as `None` are not sent. `avatar` and `banner` are file references
/// resolved like track filenames.
#[derive(Deserialize, Clone, Debug, Default)]
pub struct ProfileEdit {
    pub nick: Option<String>,
    pub avatar: Option<String>,
    pub banner: Option<String>,
}

impl ProfileEdit {
    pub fn is_empty(&self) -> bool {
        self.nick.is_none() && self.avatar.is_none() && self.banner.is_none()
    }

    pub async fn into_body(
        self,
        file_resolver: &FileResolver,
    ) -> anyhow::Result<Map<String, Value>> {
        let mut body = Map::new();
        if let Some(nick) = self.nick {
            body.insert("nick".into(), nick.into());
        }
        if let Some(avatar) = self.avatar {
            body.insert("avatar".into(), image_data(file_resolver, &avatar).await?);
        }
        if let Some(banner) = self.banner {
            body.insert("banner".into(), image_data(file_resolver, &banner).await?);
        }
        Ok(body)
    }
}

async fn image_data(file_resolver: &FileResolver, file: &str) -> anyhow::Result<Value> {
    let bytes = file_resolver
        .resolve_contents(file)
        .await
        .with_context(|| format!("Failed to load image {file}"))?;
    let mime = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        "image/png"
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        "image/jpeg"
    } else if bytes.starts_with(b"GIF8") {
        "image/gif"
    } else {
        bail!("Image {file} is not a PNG, JPEG, or GIF");
    };
    Ok(format!("data:{mime};base64,{}", BASE64_STANDARD.encode(&bytes)).into())
}
