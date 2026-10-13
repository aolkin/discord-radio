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
