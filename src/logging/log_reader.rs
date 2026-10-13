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
mod tests;
