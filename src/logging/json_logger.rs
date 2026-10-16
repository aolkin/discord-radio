use anyhow::Context;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::fs::{self, OpenOptions};
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;

/// A generic logger that appends newline-delimited JSON to a file
pub struct JsonLogger {
    file_path: PathBuf,
    lock: Arc<Mutex<()>>,
}

impl JsonLogger {
    /// Create a new JsonLogger for the specified file path
    pub fn new(file_path: PathBuf) -> Self {
        Self {
            file_path,
            lock: Arc::new(Mutex::new(())),
        }
    }

    /// Ensure the parent directory exists
    async fn ensure_directory_exists(&self) -> Result<(), std::io::Error> {
        if let Some(parent) = self.file_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        Ok(())
    }

    /// Log an entry by appending it as a JSON line to the file
    pub async fn log<T: Serialize>(&self, entry: &T) -> anyhow::Result<()> {
        // Acquire lock to ensure only one log operation at a time
        let _guard = self.lock.lock().await;

        self.ensure_directory_exists().await?;

        // Serialize to JSON
        let json = serde_json::to_string(entry).context("serializing log entry")?;

        // Open file in append mode and write the JSON line
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.file_path)
            .await?;

        file.write_all(json.as_bytes()).await?;
        file.write_all(b"\n").await?;
        file.sync_data().await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests;
