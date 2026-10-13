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
