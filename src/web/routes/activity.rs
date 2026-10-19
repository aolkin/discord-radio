use crate::state::Data;
use crate::web::error::ApiError;
use axum::Json as AxumJson;
use axum::extract::State;
use axum::http::StatusCode;
use serde::Deserialize;

// Helper function to parse activity type from string
fn parse_activity_type(type_str: &str) -> Result<crate::activity::ActivityType, ApiError> {
    match type_str.to_lowercase().as_str() {
        "listening" => Ok(crate::activity::ActivityType::Listening),
        "playing" => Ok(crate::activity::ActivityType::Playing),
        "streaming" => Ok(crate::activity::ActivityType::Streaming),
        "custom" => Ok(crate::activity::ActivityType::Custom),
        _ => Err(ApiError::bad_request(format!(
            "Unknown activity_type {type_str:?}"
        ))),
    }
}

#[derive(Debug, Deserialize)]
pub struct ActivityRequest {
    pub activity_type: String,
    pub status: String,
}

pub async fn set_bot_activity(
    State(bot_state): State<Data>,
    AxumJson(request): AxumJson<ActivityRequest>,
) -> Result<StatusCode, ApiError> {
    let activity_type = parse_activity_type(&request.activity_type)?;

    bot_state
        .activity_manager
        .set_activity(activity_type, request.status)
        .await;

    Ok(StatusCode::OK)
}
