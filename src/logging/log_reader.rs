use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::fs::File;
use tokio::io::{AsyncBufReadExt, AsyncSeekExt, BufReader};

/// A single log entry with its content and file position
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    /// The raw JSON content of the log entry
    pub content: serde_json::Value,
    /// The byte offset where this entry starts in the file
    pub start_offset: u64,
    /// The byte offset where this entry ends in the file (including newline)
    pub end_offset: u64,
}

/// Result of reading logs with pagination information
#[derive(Debug, Serialize, Deserialize)]
pub struct LogReadResult {
    /// The log entries read
    pub entries: Vec<LogEntry>,
    /// The starting offset of the first entry
    pub start_offset: u64,
    /// The ending offset of the last entry
    pub end_offset: u64,
    /// Total file size in bytes
    pub file_size: u64,
    /// Whether there are more entries before the start_offset
    pub has_previous: bool,
    /// Whether there are more entries after the end_offset
    pub has_next: bool,
}

/// A reader for newline-delimited JSON log files with support for tail and offset-based reading
pub struct LogReader {
    file_path: PathBuf,
}

/// Internal structure to track line positions without parsing
struct LinePosition {
    start_offset: u64,
    end_offset: u64,
    content: String,
}

impl LogReader {
    /// Create a new LogReader for the specified file
    pub fn new(file_path: PathBuf) -> Self {
        Self { file_path }
    }

    /// Validate that the file path is within an allowed directory (security check)
    pub fn validate_path(&self, allowed_dir: &Path) -> Result<(), std::io::Error> {
        let canonical_file = self.file_path.canonicalize().or_else(|_| {
            // If file doesn't exist yet, check the parent directory
            if let Some(parent) = self.file_path.parent() {
                parent.canonicalize()
            } else {
                Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "Invalid path",
                ))
            }
        })?;

        let canonical_allowed = allowed_dir.canonicalize()?;

        if canonical_file.starts_with(canonical_allowed) {
            Ok(())
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "Path outside allowed directory",
            ))
        }
    }

    /// Helper function to return empty result
    fn empty_result() -> LogReadResult {
        LogReadResult {
            entries: vec![],
            start_offset: 0,
            end_offset: 0,
            file_size: 0,
            has_previous: false,
            has_next: false,
        }
    }

    /// Helper function to parse lines into LogEntry objects
    fn parse_lines(lines: Vec<LinePosition>) -> Vec<LogEntry> {
        lines
            .into_iter()
            .filter_map(|line_pos| match serde_json::from_str(&line_pos.content) {
                Ok(content) => Some(LogEntry {
                    content,
                    start_offset: line_pos.start_offset,
                    end_offset: line_pos.end_offset,
                }),
                Err(e) => {
                    tracing::warn!(
                        "Failed to parse log line at offset {}: {}",
                        line_pos.start_offset,
                        e
                    );
                    None
                }
            })
            .collect()
    }

    /// Read the last n lines from the file
    pub async fn tail(
        &self,
        n: usize,
    ) -> Result<LogReadResult, Box<dyn std::error::Error + Send + Sync>> {
        if !self.file_path.exists() {
            return Ok(Self::empty_result());
        }

        let file = File::open(&self.file_path).await?;
        let file_size = file.metadata().await?.len();
        let mut reader = BufReader::new(file);

        // Read line positions without parsing
        let mut line_positions = Vec::new();
        let mut position = 0u64;

        loop {
            let start_pos = position;
            let mut line = String::new();
            let bytes_read = reader.read_line(&mut line).await?;

            if bytes_read == 0 {
                break;
            }

            position += bytes_read as u64;

            // Skip empty lines
            if line.trim().is_empty() {
                continue;
            }

            line_positions.push(LinePosition {
                start_offset: start_pos,
                end_offset: position,
                content: line,
            });
        }

        // Take the last n line positions
        let total_lines = line_positions.len();
        let start_index = total_lines.saturating_sub(n);
        let selected_positions: Vec<LinePosition> =
            line_positions.into_iter().skip(start_index).collect();

        // Now parse only the selected lines
        let entries = Self::parse_lines(selected_positions);

        let (start_offset, end_offset) = if entries.is_empty() {
            (0, 0)
        } else {
            (
                entries.first().unwrap().start_offset,
                entries.last().unwrap().end_offset,
            )
        };

        Ok(LogReadResult {
            entries,
            start_offset,
            end_offset,
            file_size,
            has_previous: start_index > 0,
            has_next: false,
        })
    }

    /// Read n lines before the given offset
    pub async fn read_before(
        &self,
        offset: u64,
        n: usize,
    ) -> Result<LogReadResult, Box<dyn std::error::Error + Send + Sync>> {
        if !self.file_path.exists() {
            return Ok(Self::empty_result());
        }

        let file = File::open(&self.file_path).await?;
        let file_size = file.metadata().await?.len();
        let mut reader = BufReader::new(file);

        // Read line positions up to the offset without parsing
        let mut line_positions = Vec::new();
        let mut position = 0u64;

        loop {
            let start_pos = position;
            let mut line = String::new();
            let bytes_read = reader.read_line(&mut line).await?;

            if bytes_read == 0 || position >= offset {
                break;
            }

            position += bytes_read as u64;

            // Skip empty lines
            if line.trim().is_empty() {
                continue;
            }

            line_positions.push(LinePosition {
                start_offset: start_pos,
                end_offset: position,
                content: line,
            });
        }

        // Take the last n line positions
        let total_lines = line_positions.len();
        let start_index = total_lines.saturating_sub(n);
        let selected_positions: Vec<LinePosition> =
            line_positions.into_iter().skip(start_index).collect();

        // Now parse only the selected lines
        let entries = Self::parse_lines(selected_positions);

        let (start_offset, end_offset) = if entries.is_empty() {
            (0, offset)
        } else {
            (
                entries.first().unwrap().start_offset,
                entries.last().unwrap().end_offset,
            )
        };

        let has_next = end_offset < file_size;

        Ok(LogReadResult {
            entries,
            start_offset,
            end_offset,
            file_size,
            has_previous: start_index > 0,
            has_next,
        })
    }

    /// Read n lines after the given offset
    pub async fn read_after(
        &self,
        offset: u64,
        n: usize,
    ) -> Result<LogReadResult, Box<dyn std::error::Error + Send + Sync>> {
        if !self.file_path.exists() {
            return Ok(Self::empty_result());
        }

        let file = File::open(&self.file_path).await?;
        let file_size = file.metadata().await?.len();
        let mut reader = BufReader::new(file);

        // Seek to the offset
        reader.seek(std::io::SeekFrom::Start(offset)).await?;

        let mut line_positions = Vec::new();
        let mut position = offset;

        while line_positions.len() < n {
            let start_pos = position;
            let mut line = String::new();
            let bytes_read = reader.read_line(&mut line).await?;

            if bytes_read == 0 {
                break;
            }

            position += bytes_read as u64;

            // Skip empty lines
            if line.trim().is_empty() {
                continue;
            }

            line_positions.push(LinePosition {
                start_offset: start_pos,
                end_offset: position,
                content: line,
            });
        }

        // Parse only the lines we collected
        let entries = Self::parse_lines(line_positions);

        let (start_offset, end_offset) = if entries.is_empty() {
            (offset, offset)
        } else {
            (
                entries.first().unwrap().start_offset,
                entries.last().unwrap().end_offset,
            )
        };

        let has_next = end_offset < file_size;

        Ok(LogReadResult {
            entries,
            start_offset,
            end_offset,
            file_size,
            has_previous: offset > 0,
            has_next,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logging::JsonLogger;
    use serde::{Deserialize, Serialize};
    use tempfile::TempDir;

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct TestEntry {
        id: u64,
        message: String,
    }

    async fn create_test_log(path: &Path, count: usize) -> Vec<TestEntry> {
        let logger = JsonLogger::new(path.to_path_buf());
        let mut entries = Vec::new();

        for i in 0..count {
            let entry = TestEntry {
                id: i as u64,
                message: format!("Message {}", i),
            };
            logger.log(&entry).await.unwrap();
            entries.push(entry);
        }

        entries
    }

    #[tokio::test]
    async fn test_tail() {
        let temp_dir = TempDir::new().unwrap();
        let log_path = temp_dir.path().join("test.jsonl");

        create_test_log(&log_path, 10).await;

        let reader = LogReader::new(log_path);
        let result = reader.tail(3).await.unwrap();

        assert_eq!(result.entries.len(), 3);
        assert!(!result.has_next);
        assert!(result.has_previous);

        // Verify last 3 entries are IDs 7, 8, 9
        for (i, entry) in result.entries.iter().enumerate() {
            let id = entry.content.get("id").unwrap().as_u64().unwrap();
            assert_eq!(id, (7 + i) as u64);
        }
    }

    #[tokio::test]
    async fn test_read_before() {
        let temp_dir = TempDir::new().unwrap();
        let log_path = temp_dir.path().join("test.jsonl");

        create_test_log(&log_path, 10).await;

        // First, tail to get the last entry's offset
        let reader = LogReader::new(log_path.clone());
        let tail_result = reader.tail(1).await.unwrap();
        let last_offset = tail_result.start_offset;

        // Now read 3 entries before the last one
        let result = reader.read_before(last_offset, 3).await.unwrap();

        assert_eq!(result.entries.len(), 3);
        assert!(result.has_previous);
        assert!(result.has_next);

        // Should get IDs 6, 7, 8
        for (i, entry) in result.entries.iter().enumerate() {
            let id = entry.content.get("id").unwrap().as_u64().unwrap();
            assert_eq!(id, (6 + i) as u64);
        }
    }

    #[tokio::test]
    async fn test_read_after() {
        let temp_dir = TempDir::new().unwrap();
        let log_path = temp_dir.path().join("test.jsonl");

        create_test_log(&log_path, 10).await;

        let reader = LogReader::new(log_path);

        // Read 3 entries from the beginning
        let result = reader.read_after(0, 3).await.unwrap();

        assert_eq!(result.entries.len(), 3);
        assert!(!result.has_previous);
        assert!(result.has_next);

        // Should get IDs 0, 1, 2
        for (i, entry) in result.entries.iter().enumerate() {
            let id = entry.content.get("id").unwrap().as_u64().unwrap();
            assert_eq!(id, i as u64);
        }
    }

    #[tokio::test]
    async fn test_validate_path() {
        let temp_dir = TempDir::new().unwrap();
        let allowed_dir = temp_dir.path();
        let log_path = allowed_dir.join("logs").join("test.jsonl");

        // Create the directory structure
        std::fs::create_dir_all(allowed_dir.join("logs")).unwrap();

        let reader = LogReader::new(log_path);

        // Should succeed - path is within allowed directory
        assert!(reader.validate_path(allowed_dir).is_ok());

        // Test with a path outside the allowed directory
        let outside_path = PathBuf::from("/tmp/outside.jsonl");
        let outside_reader = LogReader::new(outside_path);

        // Should fail - path is outside allowed directory
        assert!(outside_reader.validate_path(allowed_dir).is_err());
    }

    #[tokio::test]
    async fn test_empty_file() {
        let temp_dir = TempDir::new().unwrap();
        let log_path = temp_dir.path().join("empty.jsonl");

        // Create empty file
        std::fs::File::create(&log_path).unwrap();

        let reader = LogReader::new(log_path);
        let result = reader.tail(10).await.unwrap();

        assert_eq!(result.entries.len(), 0);
        assert!(!result.has_previous);
        assert!(!result.has_next);
    }

    #[tokio::test]
    async fn test_nonexistent_file() {
        let temp_dir = TempDir::new().unwrap();
        let log_path = temp_dir.path().join("nonexistent.jsonl");

        let reader = LogReader::new(log_path);
        let result = reader.tail(10).await.unwrap();

        assert_eq!(result.entries.len(), 0);
        assert!(!result.has_previous);
        assert!(!result.has_next);
    }
}
