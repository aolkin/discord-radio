use crate::audio::dj::config::Pool;
use crate::state::Data;
use crate::web::error::ApiError;
use crate::web::routes::profiles::{RefreshParams, registry_error};
use axum::Json as AxumJson;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use serde::Serialize;
use std::sync::Arc;

#[derive(Debug, Serialize)]
pub struct PoolsResponse {
    pub pools: Vec<String>,
}

pub async fn get_pools(State(bot_state): State<Data>) -> AxumJson<PoolsResponse> {
    let pools = bot_state.dj_config_manager.pools.list();
    AxumJson(PoolsResponse { pools })
}

pub async fn get_pool(
    State(bot_state): State<Data>,
    Path(name): Path<String>,
    Query(params): Query<RefreshParams>,
) -> Result<AxumJson<Arc<Pool>>, ApiError> {
    if params.refresh {
        return bot_state
            .dj_config_manager
            .pools
            .refresh(&name)
            .await
            .map(AxumJson)
            .map_err(registry_error);
    }
    bot_state
        .dj_config_manager
        .pools
        .get(&name)
        .map(AxumJson)
        .ok_or_else(|| ApiError::not_found(format!("No pool {name:?}")))
}

pub async fn put_pool(
    State(bot_state): State<Data>,
    Path(name): Path<String>,
    AxumJson(body): AxumJson<serde_json::Map<String, serde_json::Value>>,
) -> Result<StatusCode, ApiError> {
    bot_state
        .dj_config_manager
        .pools
        .save(&name, body)
        .await
        .map_err(registry_error)?;
    Ok(StatusCode::OK)
}
