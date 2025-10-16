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
    pub async fn log<T: Serialize>(
        &self,
        entry: &T,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Acquire lock to ensure only one log operation at a time
        let _guard = self.lock.lock().await;

        self.ensure_directory_exists().await?;

        // Serialize to JSON
        let json = serde_json::to_string(entry)?;

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
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};
    use tempfile::TempDir;

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct TestEntry {
        id: u64,
        message: String,
    }

    #[tokio::test]
    async fn test_log_entry() {
        let temp_dir = TempDir::new().unwrap();
        let log_path = temp_dir.path().join("test.jsonl");
        let logger = JsonLogger::new(log_path.clone());

        let entry1 = TestEntry {
            id: 1,
            message: "Hello".to_string(),
        };
        let entry2 = TestEntry {
            id: 2,
            message: "World".to_string(),
        };

        logger.log(&entry1).await.unwrap();
        logger.log(&entry2).await.unwrap();

        // Read the file and verify contents
        let contents = fs::read_to_string(&log_path).await.unwrap();
        let lines: Vec<&str> = contents.lines().collect();

        assert_eq!(lines.len(), 2);

        let read_entry1: TestEntry = serde_json::from_str(lines[0]).unwrap();
        let read_entry2: TestEntry = serde_json::from_str(lines[1]).unwrap();

        assert_eq!(read_entry1, entry1);
        assert_eq!(read_entry2, entry2);
    }

    #[tokio::test]
    async fn test_creates_directory() {
        let temp_dir = TempDir::new().unwrap();
        let log_path = temp_dir.path().join("subdir").join("test.jsonl");
        let logger = JsonLogger::new(log_path.clone());

        let entry = TestEntry {
            id: 1,
            message: "Test".to_string(),
        };

        logger.log(&entry).await.unwrap();

        assert!(log_path.exists());
    }
}
