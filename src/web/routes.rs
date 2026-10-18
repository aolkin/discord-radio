use crate::audio::codec::decoder::check_sample_rate;
use crate::audio::dj::config::{DJConfig, HexMessageEntry, StateWeights};
use crate::audio::dj::manager::{DJCommand, DJStateType, IdleRequest, TrackRequest};
use crate::audio::dj::segments::SignalProfilePlayback;
use crate::audio::dj::state_machine::DEFAULT_PROFILE_FADE;
use crate::audio::profiles::{ProfileStorageError, SignalProfile, is_valid_name};
use crate::bot_profile::ProfileEdit;
use crate::bucket::cache::ObjectInfo;
use crate::persistence::{DJConfigOverrideCategory, DJConfigOverrideSingle};
use crate::state::Data;
use crate::web::error::{ApiError, WithStatus};
use crate::web::state_snapshot::BotSnapshot;
use anyhow::Context;
use axum::Json as AxumJson;
use axum::extract::multipart::MultipartError;
use axum::extract::{Multipart, Path, Query, State};
use axum::http::StatusCode;
use axum::http::header::CONTENT_TYPE;
use axum::response::{Html, IntoResponse, Json};
use serde::{Deserialize, Serialize};
use serenity::model::id::{ChannelId, GuildId};
use std::time::Duration;

pub async fn index() -> Html<&'static str> {
    Html(include_str!("../../static/index.html"))
}

pub async fn logs() -> Html<&'static str> {
    Html(include_str!("../../static/logs.html"))
}

pub async fn static_file(Path(path): Path<String>) -> Result<impl IntoResponse, StatusCode> {
    let (content_type, body) = match path.as_str() {
        "base.css" => (
            "text/css; charset=utf-8",
            include_str!("../../static/base.css"),
        ),
        "logs.css" => (
            "text/css; charset=utf-8",
            include_str!("../../static/logs.css"),
        ),
        "index.css" => (
            "text/css; charset=utf-8",
            include_str!("../../static/index.css"),
        ),
        "alpine.min.js" => (
            "text/javascript; charset=utf-8",
            include_str!("../../static/alpine.min.js"),
        ),
        "index.js" => (
            "text/javascript; charset=utf-8",
            include_str!("../../static/index.js"),
        ),
        "js/activity.js" => (
            "text/javascript; charset=utf-8",
            include_str!("../../static/js/activity.js"),
        ),
        "js/api.js" => (
            "text/javascript; charset=utf-8",
            include_str!("../../static/js/api.js"),
        ),
        "js/bot-profile.js" => (
            "text/javascript; charset=utf-8",
            include_str!("../../static/js/bot-profile.js"),
        ),
        "js/channels.js" => (
            "text/javascript; charset=utf-8",
            include_str!("../../static/js/channels.js"),
        ),
        "js/connection.js" => (
            "text/javascript; charset=utf-8",
            include_str!("../../static/js/connection.js"),
        ),
        "js/dj-config.js" => (
            "text/javascript; charset=utf-8",
            include_str!("../../static/js/dj-config.js"),
        ),
        "js/dj.js" => (
            "text/javascript; charset=utf-8",
            include_str!("../../static/js/dj.js"),
        ),
        "js/file-browser.js" => (
            "text/javascript; charset=utf-8",
            include_str!("../../static/js/file-browser.js"),
        ),
        "js/modal.js" => (
            "text/javascript; charset=utf-8",
            include_str!("../../static/js/modal.js"),
        ),
        "js/run-action.js" => (
            "text/javascript; charset=utf-8",
            include_str!("../../static/js/run-action.js"),
        ),
        "js/signal-profiles.js" => (
            "text/javascript; charset=utf-8",
            include_str!("../../static/js/signal-profiles.js"),
        ),
        "js/tracks.js" => (
            "text/javascript; charset=utf-8",
            include_str!("../../static/js/tracks.js"),
        ),
        _ => return Err(StatusCode::NOT_FOUND),
    };
    Ok(([(CONTENT_TYPE, content_type)], body))
}

pub async fn get_status(State(bot_state): State<Data>) -> Json<BotSnapshot> {
    let snapshot = BotSnapshot::capture(&bot_state).await;
    Json(snapshot)
}

pub async fn health() -> impl IntoResponse {
    (StatusCode::OK, "OK")
}

#[derive(Debug, Deserialize)]
pub struct AdvanceDjStateRequest {
    pub state_type: Option<String>,
}

pub async fn advance_dj_state(
    State(bot_state): State<Data>,
    Path(guild_id): Path<GuildId>,
    AxumJson(request): AxumJson<AdvanceDjStateRequest>,
) -> Result<StatusCode, ApiError> {
    let state_type_filter = if let Some(type_str) = request.state_type {
        match type_str.to_lowercase().as_str() {
            "track" => Some(DJStateType::Track),
            "hex" | "hex_message" | "hexmessage" => Some(DJStateType::HexMessage),
            "noise" => Some(DJStateType::Noise),
            _ => {
                return Err(ApiError::bad_request(format!(
                    "Unknown state_type {type_str:?}"
                )));
            }
        }
    } else {
        None
    };

    send_dj_command(
        &bot_state,
        guild_id,
        DJCommand::ForceAdvance(state_type_filter),
    )
    .await
}

async fn send_dj_command(
    bot_state: &Data,
    guild_id: GuildId,
    command: DJCommand,
) -> Result<StatusCode, ApiError> {
    let sent = bot_state
        .send_dj_command(guild_id, command)
        .await
        .with_context(|| format!("Failed to send DJ command for guild {guild_id}"))?;
    if !sent {
        return Err(ApiError::not_found(format!("No DJ in guild {guild_id}")));
    }
    Ok(StatusCode::OK)
}

fn ensure_profile_exists(bot_state: &Data, profile: Option<&str>) -> Result<(), ApiError> {
    if let Some(profile) = profile
        && bot_state.profile_manager.get_profile(profile).is_none()
    {
        return Err(ApiError::not_found(format!(
            "No signal profile {profile:?}"
        )));
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
pub struct PlayHexMessageRequest {
    pub message: String,
}

pub async fn play_hex_message(
    State(bot_state): State<Data>,
    Path(guild_id): Path<GuildId>,
    AxumJson(request): AxumJson<PlayHexMessageRequest>,
) -> Result<StatusCode, ApiError> {
    let request = crate::audio::dj::manager::HexMessageRequest {
        message: request.message,
        volume: None,
        loops: None,
    };
    send_dj_command(&bot_state, guild_id, DJCommand::ForceHexMessage(request)).await
}

pub async fn play_track(
    State(bot_state): State<Data>,
    Path(guild_id): Path<GuildId>,
    AxumJson(request): AxumJson<TrackRequest>,
) -> Result<StatusCode, ApiError> {
    request.validate().status(StatusCode::BAD_REQUEST)?;
    bot_state
        .file_resolver
        .resolve(&request.filename)
        .await
        .map_err(ApiError::object_store)?;

    send_dj_command(&bot_state, guild_id, DJCommand::ForceTrack(request)).await
}

pub async fn force_dj_idle(
    State(bot_state): State<Data>,
    Path(guild_id): Path<GuildId>,
    AxumJson(request): AxumJson<IdleRequest>,
) -> Result<StatusCode, ApiError> {
    request.validate().status(StatusCode::BAD_REQUEST)?;
    ensure_profile_exists(&bot_state, request.noise_profile.as_deref())?;

    send_dj_command(&bot_state, guild_id, DJCommand::ForceIdle(request)).await
}

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

/// `profile: null` clears the setting; `fade_duration` (seconds) applies only
/// when setting one.
#[derive(Debug, Deserialize)]
pub struct DjProfileRequest {
    pub profile: Option<String>,
    pub fade_duration: Option<f32>,
}

pub async fn set_dj_background_profile(
    State(bot_state): State<Data>,
    Path(guild_id): Path<GuildId>,
    AxumJson(request): AxumJson<DjProfileRequest>,
) -> Result<StatusCode, ApiError> {
    set_dj_profile(
        &bot_state,
        guild_id,
        request,
        None,
        DJCommand::SetBackgroundProfile,
    )
    .await
}

pub async fn set_dj_override_profile(
    State(bot_state): State<Data>,
    Path(guild_id): Path<GuildId>,
    AxumJson(request): AxumJson<DjProfileRequest>,
) -> Result<StatusCode, ApiError> {
    set_dj_profile(
        &bot_state,
        guild_id,
        request,
        Some(DEFAULT_PROFILE_FADE),
        DJCommand::SetOverrideProfile,
    )
    .await
}

async fn set_dj_profile(
    bot_state: &Data,
    guild_id: GuildId,
    request: DjProfileRequest,
    default_fade: Option<Duration>,
    command: fn(Option<SignalProfilePlayback>) -> DJCommand,
) -> Result<StatusCode, ApiError> {
    if request.profile.is_none() && request.fade_duration.is_some() {
        return Err(ApiError::bad_request("fade_duration needs a profile"));
    }
    let fade = match request.fade_duration {
        Some(seconds) => Some(
            Duration::try_from_secs_f32(seconds)
                .context("Invalid fade_duration")
                .status(StatusCode::BAD_REQUEST)?,
        ),
        None => default_fade,
    };
    ensure_profile_exists(bot_state, request.profile.as_deref())?;

    let playback = request.profile.map(|profile| SignalProfilePlayback {
        name: profile,
        fade_in_duration: fade,
        fade_out_duration: fade,
    });
    send_dj_command(bot_state, guild_id, command(playback)).await
}

#[derive(Debug, Deserialize)]
pub struct DjChannelStatusRequest {
    pub enabled: bool,
}

pub async fn set_dj_channel_status(
    State(bot_state): State<Data>,
    Path(guild_id): Path<GuildId>,
    AxumJson(request): AxumJson<DjChannelStatusRequest>,
) -> Result<StatusCode, ApiError> {
    bot_state
        .dj_settings
        .update(guild_id, |settings| {
            settings.channel_status_disabled = !request.enabled;
            Ok(())
        })
        .await?;
    Ok(StatusCode::OK)
}

#[derive(Debug, Serialize)]
pub struct ProfilesResponse {
    pub profiles: Vec<String>,
}

pub async fn get_profiles(State(bot_state): State<Data>) -> AxumJson<ProfilesResponse> {
    let profiles = bot_state.profile_manager.list_profiles();
    AxumJson(ProfilesResponse { profiles })
}

#[derive(Debug, Deserialize)]
pub struct GetProfileParams {
    #[serde(default)]
    pub refresh: bool,
}

pub async fn get_profile(
    State(bot_state): State<Data>,
    Path(name): Path<String>,
    Query(params): Query<GetProfileParams>,
) -> Result<AxumJson<SignalProfile>, ApiError> {
    if params.refresh {
        return bot_state
            .profile_manager
            .refresh(&name)
            .await
            .map(AxumJson)
            .map_err(profile_storage_error);
    }
    bot_state
        .profile_manager
        .get_profile(&name)
        .map(AxumJson)
        .ok_or_else(|| ApiError::not_found(format!("No signal profile {name:?}")))
}

pub async fn put_profile(
    State(bot_state): State<Data>,
    Path(name): Path<String>,
    AxumJson(mut parameters): AxumJson<serde_json::Map<String, serde_json::Value>>,
) -> Result<StatusCode, ApiError> {
    parameters.insert("name".to_string(), name.into());
    let profile: SignalProfile =
        serde_json::from_value(parameters.into()).status(StatusCode::UNPROCESSABLE_ENTITY)?;
    bot_state
        .profile_manager
        .save(profile)
        .await
        .map_err(profile_storage_error)?;
    Ok(StatusCode::OK)
}

fn profile_storage_error(e: ProfileStorageError) -> ApiError {
    match e {
        ProfileStorageError::InvalidName => ApiError::new(StatusCode::BAD_REQUEST, e),
        ProfileStorageError::Storage(e) => ApiError::object_store(e),
        ProfileStorageError::Parse(_) => ApiError::new(StatusCode::BAD_GATEWAY, e),
    }
}

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

// Helper function to parse activity type from string
fn parse_activity_type(type_str: &str) -> Result<crate::activity::ActivityType, ApiError> {
    match type_str.to_lowercase().as_str() {
        "listening" => Ok(crate::activity::ActivityType::Listening),
        "playing" => Ok(crate::activity::ActivityType::Playing),
        "streaming" => Ok(crate::activity::ActivityType::Streaming),
        "custom" => Ok(crate::activity::ActivityType::Custom),
        _ => Err(ApiError::bad_request(format!(
            "Unknown activity_type {type_str:?}"
        ))),
    }
}

#[derive(Debug, Deserialize)]
pub struct ActivityRequest {
    pub activity_type: String,
    pub status: String,
}

pub async fn set_bot_activity(
    State(bot_state): State<Data>,
    AxumJson(request): AxumJson<ActivityRequest>,
) -> Result<StatusCode, ApiError> {
    let activity_type = parse_activity_type(&request.activity_type)?;

    bot_state
        .activity_manager
        .set_activity(activity_type, request.status)
        .await;

    Ok(StatusCode::OK)
}

// DJ Config Override management endpoints

#[derive(Debug, Serialize)]
pub struct DJConfigOverridesResponse {
    pub hex_messages: DJConfigOverrideCategory<HexMessageEntry>,
    pub hex_message_announcements: DJConfigOverrideCategory<String>,
    pub state_weights: DJConfigOverrideSingle<StateWeights>,
}

pub async fn get_dj_config_overrides(
    State(bot_state): State<Data>,
    Path(guild_id): Path<GuildId>,
) -> AxumJson<DJConfigOverridesResponse> {
    let settings = bot_state.dj_settings.get(guild_id).await;
    AxumJson(DJConfigOverridesResponse {
        hex_messages: settings.hex_message_overrides,
        hex_message_announcements: settings.hex_message_announcement_overrides,
        state_weights: settings.state_weight_overrides,
    })
}

fn ensure_text(text: &str) -> Result<(), ApiError> {
    if text.trim().is_empty() {
        return Err(ApiError::bad_request("text must not be empty"));
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
pub struct SetHexMessageRequest {
    pub index: Option<usize>,
    pub text: String,
    pub weight: u32,
    pub signal_profile: Option<String>,
    pub loop_min: Option<u32>,
    pub loop_max: Option<u32>,
    pub announcement: Option<String>,
}

pub async fn set_hex_message_override(
    State(bot_state): State<Data>,
    Path(guild_id): Path<GuildId>,
    AxumJson(request): AxumJson<SetHexMessageRequest>,
) -> Result<StatusCode, ApiError> {
    ensure_text(&request.text)?;

    let hex_message = HexMessageEntry {
        text: request.text,
        weight: request.weight,
        signal_profile: request.signal_profile,
        loop_min: request.loop_min,
        loop_max: request.loop_max,
        announcement: request.announcement,
    };

    bot_state
        .dj_settings
        .update(guild_id, |settings| {
            settings
                .hex_message_overrides
                .set(request.index, hex_message)
        })
        .await?;

    Ok(StatusCode::OK)
}

pub async fn delete_hex_message_override(
    State(bot_state): State<Data>,
    Path((guild_id, index)): Path<(GuildId, usize)>,
) -> Result<StatusCode, ApiError> {
    bot_state
        .dj_settings
        .update(guild_id, |settings| {
            settings.hex_message_overrides.remove(index)
        })
        .await
        .status(StatusCode::NOT_FOUND)?;

    Ok(StatusCode::OK)
}

#[derive(Debug, Deserialize)]
pub struct SetAnnouncementRequest {
    pub index: Option<usize>,
    pub text: String,
}

pub async fn set_announcement_override(
    State(bot_state): State<Data>,
    Path(guild_id): Path<GuildId>,
    AxumJson(request): AxumJson<SetAnnouncementRequest>,
) -> Result<StatusCode, ApiError> {
    ensure_text(&request.text)?;

    bot_state
        .dj_settings
        .update(guild_id, |settings| {
            settings
                .hex_message_announcement_overrides
                .set(request.index, request.text)
        })
        .await?;

    Ok(StatusCode::OK)
}

pub async fn delete_announcement_override(
    State(bot_state): State<Data>,
    Path((guild_id, index)): Path<(GuildId, usize)>,
) -> Result<StatusCode, ApiError> {
    bot_state
        .dj_settings
        .update(guild_id, |settings| {
            settings.hex_message_announcement_overrides.remove(index)
        })
        .await
        .status(StatusCode::NOT_FOUND)?;

    Ok(StatusCode::OK)
}

#[derive(Debug, Deserialize)]
pub struct ToggleOverrideCategoryRequest {
    pub category: String,
    pub enabled: bool,
}

pub async fn toggle_override_category(
    State(bot_state): State<Data>,
    Path(guild_id): Path<GuildId>,
    AxumJson(request): AxumJson<ToggleOverrideCategoryRequest>,
) -> Result<StatusCode, ApiError> {
    bot_state
        .dj_settings
        .update(guild_id, |settings| {
            let enabled = match request.category.as_str() {
                "hex_messages" => &mut settings.hex_message_overrides.enabled,
                "hex_message_announcements" => {
                    &mut settings.hex_message_announcement_overrides.enabled
                }
                "state_weights" => &mut settings.state_weight_overrides.enabled,
                _ => anyhow::bail!("Unknown category {:?}", request.category),
            };
            *enabled = request.enabled;
            Ok(())
        })
        .await
        .status(StatusCode::BAD_REQUEST)?;

    Ok(StatusCode::OK)
}

#[derive(Debug, Deserialize)]
pub struct SetStateWeightsRequest {
    pub track: u32,
    pub hex_message: u32,
    pub noise: u32,
}

pub async fn set_state_weights_override(
    State(bot_state): State<Data>,
    Path(guild_id): Path<GuildId>,
    AxumJson(request): AxumJson<SetStateWeightsRequest>,
) -> Result<StatusCode, ApiError> {
    let weights = StateWeights {
        track: request.track,
        hex_message: request.hex_message,
        noise: request.noise,
    };

    bot_state
        .dj_settings
        .update(guild_id, |settings| {
            settings.state_weight_overrides.value = Some(weights);
            Ok(())
        })
        .await?;

    Ok(StatusCode::OK)
}

pub async fn get_default_state_weights(
    State(bot_state): State<Data>,
) -> Result<AxumJson<StateWeights>, ApiError> {
    let config = DJConfig::load_from_file(&bot_state.dj_config_path("default"))
        .context("Failed to load the default DJ config")?;
    Ok(AxumJson(config.state_weights))
}

// Log retrieval endpoints

use crate::audio::tracks::get_or_create_track_manager;
use crate::logging::LogReader;

#[derive(Debug, Deserialize)]
pub struct LogQueryParams {
    /// Number of entries to retrieve (default: 50)
    #[serde(default = "default_limit")]
    limit: usize,
    /// Offset to start reading from (optional)
    offset: Option<u64>,
    /// Direction: "forward" or "backward" (default: "backward" for tail behavior)
    #[serde(default = "default_direction")]
    direction: String,
}

fn default_limit() -> usize {
    50
}

fn default_direction() -> String {
    "backward".to_string()
}

pub async fn get_logs(
    State(bot_state): State<Data>,
    Path((guild_id, log_type)): Path<(GuildId, String)>,
    Query(params): Query<LogQueryParams>,
) -> Result<AxumJson<crate::logging::LogReadResult>, ApiError> {
    // Validate log type (only allow specific log types for security)
    let log_filename = match log_type.as_str() {
        "members" => "members.jsonl",
        "dj" => "dj.jsonl",
        _ => {
            return Err(ApiError::bad_request(format!(
                "Unknown log type {log_type:?}"
            )));
        }
    };

    let log_path = bot_state
        .state_store
        .guild_logs_dir(guild_id.get())
        .join(log_filename);
    let reader = LogReader::new(log_path.clone());

    // Validate that the path is within the logs directory (security check)
    let logs_dir = bot_state.state_store.logs_dir();
    reader
        .validate_path(&logs_dir)
        .with_context(|| format!("Log {log_path:?} is outside the logs directory"))
        .status(StatusCode::FORBIDDEN)?;

    let result = match (params.offset, params.direction.as_str()) {
        (None, _) => reader.tail(params.limit).await,
        (Some(offset), "backward") => reader.read_before(offset, params.limit).await,
        (Some(offset), "forward") => reader.read_after(offset, params.limit).await,
        (Some(_), direction) => {
            return Err(ApiError::bad_request(format!(
                "Unknown direction {direction:?}"
            )));
        }
    };
    Ok(AxumJson(result.context("Failed to read logs")?))
}

// Registered Channels endpoints

#[derive(Debug, Serialize)]
pub struct RegisteredChannelResponse {
    pub channel_id: String,
    pub guild_id: String,
    pub name: String,
    pub channel_type: String,
}

#[derive(Debug, Serialize)]
pub struct RegisteredChannelsResponse {
    pub channels: Vec<RegisteredChannelResponse>,
}

pub async fn get_registered_channels(
    State(bot_state): State<Data>,
) -> Result<AxumJson<RegisteredChannelsResponse>, ApiError> {
    let channels = bot_state
        .state_store
        .load_registered_channels()
        .await
        .context("Failed to load registered channels")?;

    let response = RegisteredChannelsResponse {
        channels: channels
            .into_iter()
            .map(|c| RegisteredChannelResponse {
                channel_id: c.channel_id.to_string(),
                guild_id: c.guild_id.to_string(),
                name: c.name,
                channel_type: c.channel_type,
            })
            .collect(),
    };

    Ok(AxumJson(response))
}

#[derive(Debug, Deserialize)]
pub struct SendChannelMessageRequest {
    pub message: Option<String>,
    pub title: Option<String>,
    pub description: Option<String>,
}

pub async fn send_channel_message(
    State(bot_state): State<Data>,
    Path(channel_id): Path<ChannelId>,
    AxumJson(request): AxumJson<SendChannelMessageRequest>,
) -> Result<StatusCode, ApiError> {
    let registered = bot_state
        .state_store
        .load_registered_channels()
        .await
        .context("Failed to load registered channels")?
        .iter()
        .any(|c| c.channel_id == channel_id);
    if !registered {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            anyhow::anyhow!("Channel {channel_id} is not registered"),
        ));
    }

    let mut create_message = serenity::all::CreateMessage::new();

    if let Some(msg) = request.message {
        create_message = create_message.content(msg);
    }

    if request.title.is_some() || request.description.is_some() {
        let mut embed = serenity::all::CreateEmbed::new();

        if let Some(t) = request.title {
            embed = embed.title(t);
        }

        if let Some(d) = request.description {
            embed = embed.description(d);
        }

        create_message = create_message.embed(embed);
    }

    channel_id
        .send_message(super::http(), create_message)
        .await
        .map_err(discord_error)?;

    Ok(StatusCode::OK)
}

pub async fn get_bot_profile(
    State(bot_state): State<Data>,
    Path(guild_id): Path<GuildId>,
) -> Result<AxumJson<serde_json::Value>, ApiError> {
    let member = super::http()
        .get_member(guild_id, bot_state.bot_user_id)
        .await
        .map_err(discord_error)?;

    Ok(AxumJson(serde_json::json!({
        "nick": member.nick,
        "avatar_url": member.avatar_url(),
        "banner_url": member.banner_url(),
    })))
}

pub async fn set_bot_profile(
    State(bot_state): State<Data>,
    Path(guild_id): Path<GuildId>,
    AxumJson(edit): AxumJson<ProfileEdit>,
) -> Result<StatusCode, ApiError> {
    if edit.is_empty() {
        return Err(ApiError::bad_request(
            "Provide at least one of nick, avatar, banner",
        ));
    }
    let body = edit
        .into_body(&bot_state.file_resolver)
        .await
        .status(StatusCode::BAD_REQUEST)?;

    super::http()
        .edit_member_me(guild_id, &body, None)
        .await
        .map_err(discord_error)?;

    Ok(StatusCode::OK)
}

fn discord_error(e: serenity::Error) -> ApiError {
    if let serenity::Error::Http(http_error) = &e
        && http_error.status_code() == Some(StatusCode::TOO_MANY_REQUESTS)
    {
        return ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            anyhow::Error::new(e)
                .context("Discord is rate limiting this request; wait a bit and retry"),
        );
    }
    ApiError::new(StatusCode::BAD_GATEWAY, e)
}
