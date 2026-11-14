use crate::audio::dj::segments::{HexFolderCheck, check_audio_folder};
use crate::state::Data;
use crate::web::error::ApiError;
use crate::web::routes::files::is_plain_path;
use axum::Json as AxumJson;
use axum::extract::{Query, State};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct HexFolderParams {
    pub folder: Option<String>,
}

pub async fn check_hex_folder(
    State(bot_state): State<Data>,
    Query(params): Query<HexFolderParams>,
) -> Result<AxumJson<HexFolderCheck>, ApiError> {
    let folder = params
        .folder
        .unwrap_or_else(|| bot_state.hex_audio_folder.clone());
    let location = folder.strip_prefix("s3://").unwrap_or(&folder);
    if !is_plain_path(location) {
        return Err(ApiError::bad_request(format!(
            "Invalid hex audio folder {folder:?}"
        )));
    }
    let check = check_audio_folder(&bot_state.file_resolver, &folder)
        .await
        .map_err(ApiError::object_store)?;
    Ok(AxumJson(check))
}
