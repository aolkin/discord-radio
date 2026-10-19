use crate::audio::tracks::get_or_create_track_manager;
use crate::state::Data;
use crate::web::error::ApiError;
use axum::Json as AxumJson;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::Deserialize;
use serenity::model::id::GuildId;

#[derive(Debug, Deserialize)]
pub struct ChangeTrackStateRequest {
    pub name: String,
    pub state: Option<String>,
    pub filename: Option<String>,
    pub volume: Option<f32>,
    pub loops: Option<bool>,
    pub fade_time: Option<f32>,
}

pub async fn change_track_state(
    State(bot_state): State<Data>,
    Path(guild_id): Path<GuildId>,
    AxumJson(request): AxumJson<ChangeTrackStateRequest>,
) -> Result<StatusCode, ApiError> {
    let manager_arc = get_or_create_track_manager(&bot_state, guild_id).await;
    let mut manager = manager_arc.lock().await;
    let fade_time = request.fade_time.unwrap_or(1.0);

    let state_str = request.state.as_deref().map(|s| s.to_lowercase());
    match state_str.as_deref() {
        Some("start") => {
            let Some(filename) = request.filename else {
                return Err(ApiError::bad_request("Starting a track needs a filename"));
            };

            bot_state
                .file_resolver
                .resolve(&filename)
                .await
                .map_err(ApiError::object_store)?;
            let volume = request.volume.unwrap_or(1.0);
            let loops = request.loops.unwrap_or(true);

            manager
                .start_track(crate::audio::tracks::StartTrackArgs {
                    name: request.name,
                    filename,
                    volume,
                    fade_time,
                    loops,
                    start_position: None,
                })
                .await?;
        }
        Some("stop") => manager.stop_track(&request.name, fade_time).await?,
        None => {
            if !manager.has_track(&request.name) {
                return Err(ApiError::not_found(format!("No track {:?}", request.name)));
            }

            if let Some(new_volume) = request.volume {
                manager
                    .update_track_volume(&request.name, new_volume, fade_time)
                    .await?;
            }

            if let Some(new_loops) = request.loops {
                manager.update_track_loops(&request.name, new_loops).await?;
            }
        }
        Some(other) => return Err(ApiError::bad_request(format!("Unknown state {other:?}"))),
    }

    Ok(StatusCode::OK)
}
