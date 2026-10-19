use crate::logging::LogReader;
use crate::state::Data;
use crate::web::error::ApiError;
use anyhow::Context;
use axum::Json as AxumJson;
use axum::extract::{Path, Query, State};
use serde::Deserialize;
use serenity::model::id::GuildId;

// Log retrieval endpoints

#[derive(Debug, Deserialize)]
pub struct LogQueryParams {
    /// Number of entries to retrieve (default: 50)
    #[serde(default = "default_limit")]
    limit: usize,
    /// Offset to start reading from (optional)
    offset: Option<u64>,
    /// Direction: "forward" or "backward" (default: "backward" for tail behavior)
    #[serde(default = "default_direction")]
    direction: String,
}

fn default_limit() -> usize {
    50
}

fn default_direction() -> String {
    "backward".to_string()
}

pub async fn get_logs(
    State(bot_state): State<Data>,
    Path((guild_id, log_type)): Path<(GuildId, String)>,
    Query(params): Query<LogQueryParams>,
) -> Result<AxumJson<crate::logging::LogReadResult>, ApiError> {
    // Validate log type (only allow specific log types for security)
    let log_filename = match log_type.as_str() {
        "members" => "members.jsonl",
        "dj" => "dj.jsonl",
        _ => {
            return Err(ApiError::bad_request(format!(
                "Unknown log type {log_type:?}"
            )));
        }
    };

    let log_path = bot_state
        .state_store
        .guild_logs_dir(guild_id.get())
        .join(log_filename);
    let reader = LogReader::new(log_path);

    let result = match (params.offset, params.direction.as_str()) {
        (None, _) => reader.tail(params.limit).await,
        (Some(offset), "backward") => reader.read_before(offset, params.limit).await,
        (Some(offset), "forward") => reader.read_after(offset, params.limit).await,
        (Some(_), direction) => {
            return Err(ApiError::bad_request(format!(
                "Unknown direction {direction:?}"
            )));
        }
    };
    Ok(AxumJson(result.context("Failed to read logs")?))
}
