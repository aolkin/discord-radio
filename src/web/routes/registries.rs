use crate::audio::dj::config::{DJConfig, Pool};
use crate::audio::profiles::SignalProfile;
use crate::bucket::registry::{JsonRegistry, RegistryEntry, RegistryError};
use crate::state::Data;
use crate::web::error::ApiError;
use axum::Json as AxumJson;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;

pub trait RoutedEntry: RegistryEntry + 'static {
    const LIST_KEY: &'static str;

    fn registry(bot_state: Data) -> &'static JsonRegistry<Self>;
}

impl RoutedEntry for SignalProfile {
    const LIST_KEY: &'static str = "profiles";

    fn registry(bot_state: Data) -> &'static JsonRegistry<Self> {
        &bot_state.profile_manager
    }
}

impl RoutedEntry for DJConfig {
    const LIST_KEY: &'static str = "configs";

    fn registry(bot_state: Data) -> &'static JsonRegistry<Self> {
        &bot_state.dj_config_manager.configs
    }
}

impl RoutedEntry for Pool {
    const LIST_KEY: &'static str = "pools";

    fn registry(bot_state: Data) -> &'static JsonRegistry<Self> {
        &bot_state.dj_config_manager.pools
    }
}

#[derive(Debug, Deserialize)]
pub struct RefreshParams {
    #[serde(default)]
    pub refresh: bool,
}

pub async fn list<T: RoutedEntry>(
    State(bot_state): State<Data>,
) -> AxumJson<HashMap<&'static str, Vec<String>>> {
    AxumJson(HashMap::from([(
        T::LIST_KEY,
        T::registry(bot_state).list(),
    )]))
}

pub async fn get<T: RoutedEntry>(
    State(bot_state): State<Data>,
    Path(name): Path<String>,
    Query(params): Query<RefreshParams>,
) -> Result<AxumJson<Arc<T>>, ApiError> {
    let registry = T::registry(bot_state);
    if params.refresh {
        return registry
            .refresh(&name)
            .await
            .map(AxumJson)
            .map_err(registry_error);
    }
    registry
        .get(&name)
        .map(AxumJson)
        .ok_or_else(|| ApiError::not_found(format!("No {} {name:?}", T::KIND)))
}

pub async fn put<T: RoutedEntry>(
    State(bot_state): State<Data>,
    Path(name): Path<String>,
    AxumJson(body): AxumJson<serde_json::Map<String, serde_json::Value>>,
) -> Result<StatusCode, ApiError> {
    T::registry(bot_state)
        .save(&name, body)
        .await
        .map_err(registry_error)?;
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
