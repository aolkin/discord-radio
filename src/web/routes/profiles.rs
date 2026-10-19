use crate::audio::profiles::SignalProfile;
use crate::bucket::registry::RegistryError;
use crate::state::Data;
use crate::web::error::ApiError;
use axum::Json as AxumJson;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Serialize)]
pub struct ProfilesResponse {
    pub profiles: Vec<String>,
}

pub async fn get_profiles(State(bot_state): State<Data>) -> AxumJson<ProfilesResponse> {
    let profiles = bot_state.profile_manager.list();
    AxumJson(ProfilesResponse { profiles })
}

#[derive(Debug, Deserialize)]
pub struct RefreshParams {
    #[serde(default)]
    pub refresh: bool,
}

pub async fn get_profile(
    State(bot_state): State<Data>,
    Path(name): Path<String>,
    Query(params): Query<RefreshParams>,
) -> Result<AxumJson<Arc<SignalProfile>>, ApiError> {
    if params.refresh {
        return bot_state
            .profile_manager
            .refresh(&name)
            .await
            .map(AxumJson)
            .map_err(registry_error);
    }
    bot_state
        .profile_manager
        .get(&name)
        .map(AxumJson)
        .ok_or_else(|| ApiError::not_found(format!("No signal profile {name:?}")))
}

pub async fn put_profile(
    State(bot_state): State<Data>,
    Path(name): Path<String>,
    AxumJson(body): AxumJson<serde_json::Map<String, serde_json::Value>>,
) -> Result<StatusCode, ApiError> {
    bot_state
        .profile_manager
        .save(&name, body)
        .await
        .map_err(registry_error)?;
    Ok(StatusCode::OK)
}

pub fn registry_error(e: RegistryError) -> ApiError {
    match e {
        RegistryError::InvalidName => ApiError::new(StatusCode::BAD_REQUEST, e),
        RegistryError::Invalid(e) => ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, e),
        RegistryError::Storage(e) => ApiError::object_store(e),
        RegistryError::Parse(e) => ApiError::new(StatusCode::BAD_GATEWAY, e),
    }
}
