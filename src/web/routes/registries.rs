use crate::audio::dj::config::{DJConfig, Pool};
use crate::audio::profiles::SignalProfile;
use crate::bucket::registry::{JsonRegistry, RegistryEntry, RegistryError};
use crate::state::Data;
use crate::web::error::{ApiError, WithStatus};
use axum::Json as AxumJson;
use axum::Router;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::get;
use serde::Deserialize;
use std::sync::Arc;

pub trait RoutedEntry: RegistryEntry + Send + Sync + 'static {
    const PATH: &'static str;

    fn registry(bot_state: Data) -> &'static JsonRegistry<Self>;

    fn validate_body(
        _bot_state: &Data,
        _body: &serde_json::Map<String, serde_json::Value>,
    ) -> Result<(), ApiError> {
        Ok(())
    }
}

impl RoutedEntry for SignalProfile {
    const PATH: &'static str = "/api/profiles";

    fn registry(bot_state: Data) -> &'static JsonRegistry<Self> {
        &bot_state.profile_manager
    }
}

impl RoutedEntry for DJConfig {
    const PATH: &'static str = "/api/dj-configs";

    fn registry(bot_state: Data) -> &'static JsonRegistry<Self> {
        &bot_state.dj_config_manager.configs
    }

    fn validate_body(
        bot_state: &Data,
        body: &serde_json::Map<String, serde_json::Value>,
    ) -> Result<(), ApiError> {
        if let Some(pools) = body.get("pools").and_then(|pools| pools.as_object()) {
            for pool in pools.keys() {
                bot_state
                    .dj_config_manager
                    .pools
                    .require(pool)
                    .status(StatusCode::UNPROCESSABLE_ENTITY)?;
            }
        }
        Ok(())
    }
}

impl RoutedEntry for Pool {
    const PATH: &'static str = "/api/pools";

    fn registry(bot_state: Data) -> &'static JsonRegistry<Self> {
        &bot_state.dj_config_manager.pools
    }
}

#[derive(Debug, Deserialize)]
pub struct RefreshParams {
    #[serde(default)]
    pub refresh: bool,
}

pub async fn list<T: RoutedEntry>(State(bot_state): State<Data>) -> AxumJson<Vec<String>> {
    AxumJson(T::registry(bot_state).list())
}

pub async fn get_entry<T: RoutedEntry>(
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
    T::validate_body(&bot_state, &body)?;
    T::registry(bot_state)
        .save(&name, body)
        .await
        .map_err(registry_error)?;
    Ok(StatusCode::OK)
}

pub async fn post<T: RoutedEntry>(
    State(bot_state): State<Data>,
    Path(name): Path<String>,
    AxumJson(body): AxumJson<serde_json::Map<String, serde_json::Value>>,
) -> Result<StatusCode, ApiError> {
    if T::registry(bot_state).get(&name).is_some() {
        return Err(ApiError::conflict(format!(
            "A {} named {name:?} already exists",
            T::KIND
        )));
    }
    put::<T>(State(bot_state), Path(name), AxumJson(body)).await
}

pub fn routes<T: RoutedEntry>() -> Router<Data> {
    Router::new().route(T::PATH, get(list::<T>)).route(
        &format!("{}/{{name}}", T::PATH),
        get(get_entry::<T>).put(put::<T>).post(post::<T>),
    )
}

fn registry_error(e: RegistryError) -> ApiError {
    match e {
        RegistryError::InvalidName => ApiError::new(StatusCode::BAD_REQUEST, e),
        RegistryError::Invalid(e) => ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, e),
        RegistryError::Storage(e) => ApiError::object_store(e),
        RegistryError::Parse(e) => ApiError::new(StatusCode::BAD_GATEWAY, e),
    }
}
