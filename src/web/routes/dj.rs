use crate::audio::dj::manager::{DJCommand, DJStateType, IdleRequest, TrackRequest};
use crate::audio::dj::segments::SignalProfilePlayback;
use crate::audio::dj::state_machine::DEFAULT_PROFILE_FADE;
use crate::audio::tracks::get_or_create_track_manager;
use crate::state::Data;
use crate::web::error::{ApiError, WithStatus};
use anyhow::Context;
use axum::Json as AxumJson;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::Deserialize;
use serenity::model::id::GuildId;
use std::time::Duration;

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
