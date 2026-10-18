use crate::audio::dj::config::DJConfig;
use crate::audio::dj::config::storage::DjConfigStorageError;
use crate::state::Data;
use crate::web::error::{ApiError, WithStatus};
use crate::web::routes::profiles::RefreshParams;
use axum::Json as AxumJson;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use serde::Serialize;
use std::sync::Arc;

#[derive(Debug, Serialize)]
pub struct DjConfigsResponse {
    pub configs: Vec<String>,
}

pub async fn get_dj_configs(State(bot_state): State<Data>) -> AxumJson<DjConfigsResponse> {
    let configs = bot_state.dj_config_manager.list();
    AxumJson(DjConfigsResponse { configs })
}

pub async fn get_dj_config(
    State(bot_state): State<Data>,
    Path(name): Path<String>,
    Query(params): Query<RefreshParams>,
) -> Result<AxumJson<Arc<DJConfig>>, ApiError> {
    if params.refresh {
        return bot_state
            .dj_config_manager
            .refresh(&name)
            .await
            .map(AxumJson)
            .map_err(dj_config_storage_error);
    }
    bot_state
        .dj_config_manager
        .get(&name)
        .map(AxumJson)
        .ok_or_else(|| ApiError::not_found(format!("No DJ config {name:?}")))
}

pub async fn put_dj_config(
    State(bot_state): State<Data>,
    Path(name): Path<String>,
    AxumJson(mut config): AxumJson<serde_json::Map<String, serde_json::Value>>,
) -> Result<StatusCode, ApiError> {
    config.insert("name".to_string(), name.into());
    let config: DJConfig =
        serde_json::from_value(config.into()).status(StatusCode::UNPROCESSABLE_ENTITY)?;
    config.validate().status(StatusCode::UNPROCESSABLE_ENTITY)?;
    bot_state
        .dj_config_manager
        .save(config)
        .await
        .map_err(dj_config_storage_error)?;
    Ok(StatusCode::OK)
}

fn dj_config_storage_error(e: DjConfigStorageError) -> ApiError {
    match e {
        DjConfigStorageError::InvalidName => ApiError::new(StatusCode::BAD_REQUEST, e),
        DjConfigStorageError::Storage(e) => ApiError::object_store(e),
        DjConfigStorageError::Parse(_) => ApiError::new(StatusCode::BAD_GATEWAY, e),
    }
}
