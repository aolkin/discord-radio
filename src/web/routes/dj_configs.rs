use crate::audio::dj::config::DJConfig;
use crate::state::Data;
use crate::web::error::ApiError;
use crate::web::routes::profiles::{RefreshParams, registry_error};
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
    let configs = bot_state.dj_config_manager.configs.list();
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
            .configs
            .refresh(&name)
            .await
            .map(AxumJson)
            .map_err(registry_error);
    }
    bot_state
        .dj_config_manager
        .configs
        .get(&name)
        .map(AxumJson)
        .ok_or_else(|| ApiError::not_found(format!("No DJ config {name:?}")))
}

pub async fn put_dj_config(
    State(bot_state): State<Data>,
    Path(name): Path<String>,
    AxumJson(body): AxumJson<serde_json::Map<String, serde_json::Value>>,
) -> Result<StatusCode, ApiError> {
    bot_state
        .dj_config_manager
        .configs
        .save(&name, body)
        .await
        .map_err(registry_error)?;
    Ok(StatusCode::OK)
}
