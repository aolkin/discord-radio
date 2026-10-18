mod json_logger;
mod log_reader;

pub use json_logger::JsonLogger;
pub use log_reader::{LogReadResult, LogReader};

use anyhow::Context;
use serde::Serialize;
use std::path::{Path, PathBuf};

/// Get the logs directory path for a given base path
pub fn logs_dir(base_path: &Path) -> PathBuf {
    base_path.join("logs")
}

/// Get the logs directory path for a specific guild
pub fn guild_logs_dir(base_path: &Path, guild_id: u64) -> PathBuf {
    logs_dir(base_path).join(guild_id.to_string())
}

pub async fn log_member_activity(
    base_path: &Path,
    guild_id: u64,
    user_id: u64,
    username: &str,
    nickname: Option<&str>,
    action: &str,
    channel_id: Option<u64>,
) -> anyhow::Result<()> {
    #[derive(Serialize)]
    struct MemberActivityEntry<'a> {
        timestamp: String,
        user_id: u64,
        username: &'a str,
        #[serde(skip_serializing_if = "Option::is_none")]
        nickname: Option<&'a str>,
        action: &'a str,
        #[serde(skip_serializing_if = "Option::is_none")]
        channel_id: Option<u64>,
    }

    let logger = JsonLogger::new(guild_logs_dir(base_path, guild_id).join("members.jsonl"));
    let entry = MemberActivityEntry {
        timestamp: chrono::Utc::now().to_rfc3339(),
        user_id,
        username,
        nickname,
        action,
        channel_id,
    };

    logger
        .log(&entry)
        .await
        .with_context(|| format!("Failed to log member activity in guild {guild_id}"))
}

pub async fn log_dj_activity(
    base_path: &Path,
    guild_id: u64,
    event_type: String,
    details: serde_json::Value,
) -> anyhow::Result<()> {
    #[derive(Serialize)]
    struct DJActivityEntry {
        timestamp: String,
        event_type: String,
        details: serde_json::Value,
    }

    let logger = JsonLogger::new(guild_logs_dir(base_path, guild_id).join("dj.jsonl"));
    let entry = DJActivityEntry {
        timestamp: chrono::Utc::now().to_rfc3339(),
        event_type,
        details,
    };

    logger
        .log(&entry)
        .await
        .with_context(|| format!("Failed to log DJ activity in guild {guild_id}"))
}
