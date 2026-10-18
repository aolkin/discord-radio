use crate::audio::codec::decoder::check_sample_rate;
use crate::bucket::cache::ObjectInfo;
use crate::bucket::registry::is_valid_name;
use crate::state::Data;
use crate::web::error::{ApiError, WithStatus};
use anyhow::Context;
use axum::Json as AxumJson;
use axum::extract::multipart::MultipartError;
use axum::extract::{Multipart, Query, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};

const AUDIO_ROOT: &str = "tracks/";
const IMAGE_ROOT: &str = "images/";
const REQUIRED_SAMPLE_RATE: u32 = 48000;
pub const MAX_UPLOAD_BYTES: usize = 100 * 1024 * 1024;

/// The root that `prefix` sits under, if `prefix` is a valid folder to upload
/// into or create folders in.
fn upload_root(prefix: &str) -> Option<&'static str> {
    let root = [AUDIO_ROOT, IMAGE_ROOT]
        .into_iter()
        .find(|root| prefix.starts_with(root))?;
    let valid = prefix.ends_with('/')
        && prefix
            .trim_end_matches('/')
            .split('/')
            .all(|segment| !segment.is_empty() && segment != "..");
    valid.then_some(root)
}

fn invalid_prefix(prefix: &str) -> ApiError {
    ApiError::bad_request(format!(
        "Invalid prefix {prefix:?}; it must be a folder under {AUDIO_ROOT} or {IMAGE_ROOT}"
    ))
}

#[derive(Debug, Deserialize)]
pub struct BrowseFilesParams {
    pub prefix: String,
}

#[derive(Debug, Serialize)]
pub struct BrowseFilesResponse {
    pub prefix: String,
    pub folders: Vec<String>,
    pub files: Vec<ObjectInfo>,
}

pub async fn browse_files(
    State(bot_state): State<Data>,
    Query(params): Query<BrowseFilesParams>,
) -> Result<AxumJson<BrowseFilesResponse>, ApiError> {
    let listing = bot_state
        .file_resolver
        .bucket()
        .map_err(ApiError::object_store)?
        .list_dir(&params.prefix)
        .await
        .map_err(ApiError::object_store)?;
    Ok(AxumJson(BrowseFilesResponse {
        prefix: params.prefix,
        folders: listing.folders,
        files: listing.files,
    }))
}

#[derive(Debug, Serialize)]
pub struct FileKeyResponse {
    pub key: String,
}

pub async fn upload_file(
    State(bot_state): State<Data>,
    Query(params): Query<BrowseFilesParams>,
    mut multipart: Multipart,
) -> Result<AxumJson<FileKeyResponse>, ApiError> {
    let root = upload_root(&params.prefix).ok_or_else(|| invalid_prefix(&params.prefix))?;

    let multipart_error = |e: MultipartError| ApiError::new(e.status(), e);
    let (filename, bytes) = loop {
        let field = multipart
            .next_field()
            .await
            .map_err(multipart_error)?
            .ok_or_else(|| ApiError::bad_request("Missing \"file\" field"))?;
        if field.name() == Some("file") {
            let filename = field.file_name().map(str::to_string);
            let bytes = field.bytes().await.map_err(multipart_error)?;
            break (filename, bytes);
        }
    };

    let filename = filename
        .filter(|name| !name.is_empty() && !name.contains(['/', '\\']) && name != "..")
        .ok_or_else(|| ApiError::bad_request("The file needs a plain filename"))?;
    let extension = filename
        .rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_lowercase())
        .unwrap_or_default();
    let content_type = if root == AUDIO_ROOT {
        let (extension, content_type) = match extension.as_str() {
            "mp3" => ("mp3", "audio/mpeg"),
            "ogg" => ("ogg", "audio/ogg"),
            "wav" => ("wav", "audio/wav"),
            _ => {
                return Err(ApiError::bad_request(
                    "Only .mp3, .ogg and .wav files are accepted",
                ));
            }
        };
        let check_bytes = bytes.to_vec();
        tokio::task::spawn_blocking(move || {
            check_sample_rate(check_bytes, extension, REQUIRED_SAMPLE_RATE)
        })
        .await
        .context("Sample rate check failed to run")?
        .status(StatusCode::UNPROCESSABLE_ENTITY)?;
        content_type
    } else {
        match extension.as_str() {
            "png" => "image/png",
            "jpg" | "jpeg" => "image/jpeg",
            "gif" => "image/gif",
            _ => {
                return Err(ApiError::bad_request(
                    "Only .png, .jpg and .gif files are accepted",
                ));
            }
        }
    };

    let key = format!("{}{filename}", params.prefix);
    bot_state
        .file_resolver
        .bucket()
        .map_err(ApiError::object_store)?
        .upload(&key, &bytes, content_type)
        .await
        .map_err(ApiError::object_store)?;
    Ok(AxumJson(FileKeyResponse { key }))
}

#[derive(Debug, Deserialize)]
pub struct CreateFolderRequest {
    pub prefix: String,
    pub name: String,
}

pub async fn create_folder(
    State(bot_state): State<Data>,
    AxumJson(request): AxumJson<CreateFolderRequest>,
) -> Result<AxumJson<FileKeyResponse>, ApiError> {
    if upload_root(&request.prefix).is_none() {
        return Err(invalid_prefix(&request.prefix));
    }
    if !is_valid_name(&request.name) {
        return Err(ApiError::bad_request(
            "Folder names may contain only letters, digits, - and _",
        ));
    }

    let key = format!("{}{}/", request.prefix, request.name);
    bot_state
        .file_resolver
        .bucket()
        .map_err(ApiError::object_store)?
        .upload(&key, &[], "application/x-directory")
        .await
        .map_err(ApiError::object_store)?;
    Ok(AxumJson(FileKeyResponse { key }))
}
