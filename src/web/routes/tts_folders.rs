use crate::audio::dj::tts::{TtsEncoding, list_missing_files};
use crate::bucket::with_trailing_slash;
use crate::state::Data;
use crate::web::error::ApiError;
use crate::web::routes::files::is_plain_path;
use axum::Json as AxumJson;
use axum::extract::{Query, State};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Serialize, Deserialize)]
pub struct TtsFolderCheck {
    pub folder: String,
    pub missing_files: Vec<String>,
}

pub async fn check_tts_folders(
    State(bot_state): State<Data>,
    Query(overrides): Query<BTreeMap<TtsEncoding, String>>,
) -> Result<AxumJson<BTreeMap<TtsEncoding, TtsFolderCheck>>, ApiError> {
    let mut checks = BTreeMap::new();
    for encoding in [TtsEncoding::Hex] {
        let folder = with_trailing_slash(
            overrides
                .get(&encoding)
                .unwrap_or(&bot_state.hex_audio_folder),
        );
        let location = folder.strip_prefix("s3://").unwrap_or(&folder);
        if !is_plain_path(location) {
            return Err(ApiError::bad_request(format!(
                "Invalid {} audio folder {folder:?}",
                encoding.name()
            )));
        }
        let missing_files = list_missing_files(&bot_state.file_resolver, encoding, &folder)
            .await
            .map_err(ApiError::object_store)?;
        checks.insert(
            encoding,
            TtsFolderCheck {
                folder,
                missing_files,
            },
        );
    }
    Ok(AxumJson(checks))
}
