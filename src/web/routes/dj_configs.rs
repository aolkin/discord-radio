use crate::audio::dj::config::DJConfig;
use crate::state::Data;
use crate::web::error::ApiError;
use crate::web::routes::profiles::{RefreshParams, get_registry_entry, put_registry_entry};
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
    get_registry_entry(&bot_state.dj_config_manager, &name, params).await
}

pub async fn put_dj_config(
    State(bot_state): State<Data>,
    Path(name): Path<String>,
    AxumJson(body): AxumJson<serde_json::Map<String, serde_json::Value>>,
) -> Result<StatusCode, ApiError> {
    put_registry_entry(&bot_state.dj_config_manager, &name, body).await
}
