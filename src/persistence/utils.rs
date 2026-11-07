use anyhow::Context;
use serde::{Serialize, de::DeserializeOwned};
use std::path::{Path, PathBuf};
use tokio::io::AsyncWriteExt;

pub async fn read_json<T: DeserializeOwned + Default>(path: &Path) -> anyhow::Result<T> {
    let content = match tokio::fs::read_to_string(path).await {
        Ok(content) => content,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(T::default()),
        Err(e) => return Err(e).with_context(|| format!("reading {path:?}")),
    };
    serde_json::from_str(&content).with_context(|| format!("parsing {path:?}"))
}

pub async fn write_json<T: Serialize>(data: &T, path: &Path) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .with_context(|| format!("creating {parent:?}"))?;
    }

    let mut temp_name = path.as_os_str().to_owned();
    temp_name.push(".tmp");
    let temp_path = PathBuf::from(temp_name);

    let json = serde_json::to_string_pretty(data).context("serializing JSON")?;
    let mut file = tokio::fs::File::create(&temp_path)
        .await
        .with_context(|| format!("creating {temp_path:?}"))?;
    file.write_all(json.as_bytes())
        .await
        .with_context(|| format!("writing {temp_path:?}"))?;
    file.sync_all()
        .await
        .with_context(|| format!("syncing {temp_path:?}"))?;
    drop(file);
    tokio::fs::rename(&temp_path, path)
        .await
        .with_context(|| format!("renaming {temp_path:?} to {path:?}"))?;

    Ok(())
}
