use crate::audio::profiles::{ProfileStorageError, SignalProfile};
use crate::state::Data;
use crate::web::error::{ApiError, WithStatus};
use axum::Json as AxumJson;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
pub struct ProfilesResponse {
    pub profiles: Vec<String>,
}

pub async fn get_profiles(State(bot_state): State<Data>) -> AxumJson<ProfilesResponse> {
    let profiles = bot_state.profile_manager.list_profiles();
    AxumJson(ProfilesResponse { profiles })
}

#[derive(Debug, Deserialize)]
pub struct GetProfileParams {
    #[serde(default)]
    pub refresh: bool,
}

pub async fn get_profile(
    State(bot_state): State<Data>,
    Path(name): Path<String>,
    Query(params): Query<GetProfileParams>,
) -> Result<AxumJson<SignalProfile>, ApiError> {
    if params.refresh {
        return bot_state
            .profile_manager
            .refresh(&name)
            .await
            .map(AxumJson)
            .map_err(profile_storage_error);
    }
    bot_state
        .profile_manager
        .get_profile(&name)
        .map(AxumJson)
        .ok_or_else(|| ApiError::not_found(format!("No signal profile {name:?}")))
}

pub async fn put_profile(
    State(bot_state): State<Data>,
    Path(name): Path<String>,
    AxumJson(mut parameters): AxumJson<serde_json::Map<String, serde_json::Value>>,
) -> Result<StatusCode, ApiError> {
    parameters.insert("name".to_string(), name.into());
    let profile: SignalProfile =
        serde_json::from_value(parameters.into()).status(StatusCode::UNPROCESSABLE_ENTITY)?;
    bot_state
        .profile_manager
        .save(profile)
        .await
        .map_err(profile_storage_error)?;
    Ok(StatusCode::OK)
}

fn profile_storage_error(e: ProfileStorageError) -> ApiError {
    match e {
        ProfileStorageError::InvalidName => ApiError::new(StatusCode::BAD_REQUEST, e),
        ProfileStorageError::Storage(e) => ApiError::object_store(e),
        ProfileStorageError::Parse(_) => ApiError::new(StatusCode::BAD_GATEWAY, e),
    }
}
