use crate::activity::ActivityType;
use crate::state::Data;
use crate::web::error::ApiError;
use axum::Json as AxumJson;
use axum::extract::State;
use axum::http::StatusCode;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct ActivityRequest {
    pub activity_type: ActivityType,
    pub status: String,
}

pub async fn set_bot_activity(
    State(bot_state): State<Data>,
    AxumJson(request): AxumJson<ActivityRequest>,
) -> Result<StatusCode, ApiError> {
    bot_state
        .activity_manager
        .set_activity(request.activity_type, request.status)
        .await;

    Ok(StatusCode::OK)
}
