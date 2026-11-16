use crate::audio::tracks::{ChangeTrackStateError, ChangeTrackStateRequest};
use crate::state::Data;
use crate::web::error::ApiError;
use axum::Json as AxumJson;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serenity::model::id::GuildId;

pub async fn change_track_state(
    State(bot_state): State<Data>,
    Path(guild_id): Path<GuildId>,
    AxumJson(request): AxumJson<ChangeTrackStateRequest>,
) -> Result<StatusCode, ApiError> {
    bot_state
        .require_radio(guild_id)
        .map_err(ApiError::inactive)?
        .track_manager
        .change_state(request)
        .await
        .map_err(|e| match e {
            ChangeTrackStateError::MissingFilename => ApiError::new(StatusCode::BAD_REQUEST, e),
            ChangeTrackStateError::TrackNotFound(_) => ApiError::new(StatusCode::NOT_FOUND, e),
            ChangeTrackStateError::Resolve(e) => ApiError::object_store(e),
            ChangeTrackStateError::Failed(e) => e.into(),
        })?;

    Ok(StatusCode::OK)
}
