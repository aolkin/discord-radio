use crate::audio::dj::segments::hex::{list_missing_hex_files, normalize_audio_folder};
use crate::state::Data;
use crate::web::error::ApiError;
use crate::web::routes::files::is_plain_path;
use axum::Json as AxumJson;
use axum::extract::{Query, State};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct HexFolderParams {
    pub folder: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct HexFolderResponse {
    pub folder: String,
    pub missing_files: Vec<String>,
}

pub async fn check_hex_folder(
    State(bot_state): State<Data>,
    Query(params): Query<HexFolderParams>,
) -> Result<AxumJson<HexFolderResponse>, ApiError> {
    let folder = normalize_audio_folder(
        params
            .folder
            .as_deref()
            .unwrap_or(&bot_state.hex_audio_folder),
    );
    let location = folder.strip_prefix("s3://").unwrap_or(&folder);
    if !is_plain_path(location) {
        return Err(ApiError::bad_request(format!(
            "Invalid hex audio folder {folder:?}"
        )));
    }
    let missing_files = list_missing_hex_files(&bot_state.file_resolver, &folder)
        .await
        .map_err(ApiError::object_store)?;
    Ok(AxumJson(HexFolderResponse {
        folder,
        missing_files,
    }))
}
