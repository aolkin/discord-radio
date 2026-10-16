use anyhow::Context;
use std::path::Path;

/// Helper function to save JSON data to a file atomically
pub async fn save_json_to_file<T: serde::Serialize>(data: &T, path: &Path) -> anyhow::Result<()> {
    let temp_path = path.with_extension("json.tmp");

    // Ensure directory exists
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .with_context(|| format!("creating {parent:?}"))?;
    }

    let json = serde_json::to_string_pretty(data).context("serializing JSON")?;
    tokio::fs::write(&temp_path, json)
        .await
        .with_context(|| format!("writing {temp_path:?}"))?;
    tokio::fs::rename(&temp_path, path)
        .await
        .with_context(|| format!("renaming {temp_path:?} to {path:?}"))?;

    Ok(())
}
