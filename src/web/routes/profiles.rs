use crate::audio::profiles::SignalProfile;
use crate::bucket::registry::{JsonRegistry, RegistryEntry, RegistryError};
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
    get_registry_entry(&bot_state.profile_manager, &name, params).await
}

pub async fn put_profile(
    State(bot_state): State<Data>,
    Path(name): Path<String>,
    AxumJson(body): AxumJson<serde_json::Map<String, serde_json::Value>>,
) -> Result<StatusCode, ApiError> {
    put_registry_entry(&bot_state.profile_manager, &name, body).await
}

pub async fn get_registry_entry<T: RegistryEntry>(
    registry: &JsonRegistry<T>,
    name: &str,
    params: RefreshParams,
) -> Result<AxumJson<Arc<T>>, ApiError> {
    if params.refresh {
        return registry
            .refresh(name)
            .await
            .map(AxumJson)
            .map_err(registry_error);
    }
    registry
        .get(name)
        .map(AxumJson)
        .ok_or_else(|| ApiError::not_found(format!("No {} {name:?}", T::KIND)))
}

pub async fn put_registry_entry<T: RegistryEntry>(
    registry: &JsonRegistry<T>,
    name: &str,
    body: serde_json::Map<String, serde_json::Value>,
) -> Result<StatusCode, ApiError> {
    registry.save(name, body).await.map_err(registry_error)?;
    Ok(StatusCode::OK)
}

fn registry_error(e: RegistryError) -> ApiError {
    match e {
        RegistryError::InvalidName => ApiError::new(StatusCode::BAD_REQUEST, e),
        RegistryError::Invalid(e) => ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, e),
        RegistryError::Storage(e) => ApiError::object_store(e),
        RegistryError::Parse(e) => ApiError::new(StatusCode::BAD_GATEWAY, e),
    }
}
