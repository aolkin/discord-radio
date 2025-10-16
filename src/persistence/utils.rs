/// Helper function to save JSON data to a file atomically
pub async fn save_json_to_file<T: serde::Serialize>(
    data: &T,
    path: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let temp_path = path.with_extension("json.tmp");

    // Ensure directory exists
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    let json = serde_json::to_string_pretty(data)?;
    tokio::fs::write(&temp_path, json).await?;
    tokio::fs::rename(&temp_path, path).await?;

    Ok(())
}
