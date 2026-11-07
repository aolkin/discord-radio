use crate::audio::dj::manager::{DJCommand, DjBusy, IdleRequest, TrackRequest};
use crate::audio::dj::segments::SignalProfilePlayback;
use crate::audio::dj::state_machine::{ChannelStatus, DEFAULT_PROFILE_FADE};
use crate::audio::dj::tiered::TimedValue;
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
    if let Some(pool) = &request.state_type {
        bot_state
            .dj_config_manager
            .pools
            .require(pool)
            .status(StatusCode::BAD_REQUEST)?;
    }
    send_dj_command(
        &bot_state,
        guild_id,
        DJCommand::ForceAdvance(request.state_type),
    )
}

fn send_dj_command(
    bot_state: &Data,
    guild_id: GuildId,
    command: DJCommand,
) -> Result<StatusCode, ApiError> {
    let sent = bot_state.dj_manager(guild_id).send(command).map_err(|e| {
        let status = if e.is::<DjBusy>() {
            StatusCode::SERVICE_UNAVAILABLE
        } else {
            StatusCode::INTERNAL_SERVER_ERROR
        };
        ApiError::new(
            status,
            e.context(format!("Failed to send DJ command for guild {guild_id}")),
        )
    })?;
    if !sent {
        return Err(ApiError::not_found(format!("No DJ in guild {guild_id}")));
    }
    Ok(StatusCode::OK)
}

fn ensure_profile_exists(bot_state: &Data, profile: Option<&str>) -> Result<(), ApiError> {
    if let Some(profile) = profile
        && bot_state.profile_manager.get(profile).is_none()
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
    pub pool: Option<String>,
}

pub async fn play_hex_message(
    State(bot_state): State<Data>,
    Path(guild_id): Path<GuildId>,
    AxumJson(request): AxumJson<PlayHexMessageRequest>,
) -> Result<StatusCode, ApiError> {
    if let Some(pool) = &request.pool {
        bot_state
            .dj_config_manager
            .pools
            .require(pool)
            .status(StatusCode::BAD_REQUEST)?;
    }
    let request = crate::audio::dj::manager::HexMessageRequest {
        message: request.message,
        volume: None,
        loops: None,
        pool: request.pool,
    };
    send_dj_command(&bot_state, guild_id, DJCommand::ForceHexMessage(request))
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

    send_dj_command(&bot_state, guild_id, DJCommand::ForceTrack(request))
}

pub async fn force_dj_idle(
    State(bot_state): State<Data>,
    Path(guild_id): Path<GuildId>,
    AxumJson(request): AxumJson<IdleRequest>,
) -> Result<StatusCode, ApiError> {
    request.validate().status(StatusCode::BAD_REQUEST)?;
    ensure_profile_exists(&bot_state, request.noise_profile.as_deref())?;

    send_dj_command(&bot_state, guild_id, DJCommand::ForceIdle(request))
}

/// `profile: null` clears the setting; `fade_duration` and `duration`
/// (seconds) apply only when setting one.
#[derive(Debug, Deserialize)]
pub struct DjProfileRequest {
    pub profile: Option<String>,
    pub fade_duration: Option<f32>,
    pub duration: Option<f32>,
}

fn optional_seconds(seconds: Option<f32>, name: &str) -> Result<Option<Duration>, ApiError> {
    seconds
        .map(|seconds| {
            Duration::try_from_secs_f32(seconds)
                .with_context(|| format!("Invalid {name}"))
                .status(StatusCode::BAD_REQUEST)
        })
        .transpose()
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
    command: fn(Option<TimedValue<SignalProfilePlayback>>) -> DJCommand,
) -> Result<StatusCode, ApiError> {
    if request.profile.is_none() && (request.fade_duration.is_some() || request.duration.is_some())
    {
        return Err(ApiError::bad_request(
            "fade_duration and duration need a profile",
        ));
    }
    let fade = optional_seconds(request.fade_duration, "fade_duration")?.or(default_fade);
    let duration = optional_seconds(request.duration, "duration")?;
    ensure_profile_exists(bot_state, request.profile.as_deref())?;

    let playback = request.profile.map(|profile| {
        TimedValue::new(
            SignalProfilePlayback {
                name: profile,
                fade_in_duration: fade,
                fade_out_duration: fade,
            },
            duration,
        )
    });
    send_dj_command(bot_state, guild_id, command(playback))
}

#[derive(Debug, Deserialize)]
pub struct DjChannelStatusRequest<T> {
    pub status: Option<T>,
    pub duration: Option<f32>,
}

impl<T> DjChannelStatusRequest<T> {
    fn timed(
        self,
        into_status: impl FnOnce(T) -> ChannelStatus,
    ) -> Result<Option<TimedValue<ChannelStatus>>, ApiError> {
        if self.status.is_none() && self.duration.is_some() {
            return Err(ApiError::bad_request("duration needs a status"));
        }
        let duration = optional_seconds(self.duration, "duration")?;
        Ok(self
            .status
            .map(|status| TimedValue::new(into_status(status), duration)))
    }
}

pub async fn set_dj_background_channel_status(
    State(bot_state): State<Data>,
    Path(guild_id): Path<GuildId>,
    AxumJson(request): AxumJson<DjChannelStatusRequest<String>>,
) -> Result<StatusCode, ApiError> {
    let status = request.timed(ChannelStatus::Fixed)?;
    send_dj_command(
        &bot_state,
        guild_id,
        DJCommand::SetBackgroundChannelStatus(status),
    )
}

pub async fn set_dj_override_channel_status(
    State(bot_state): State<Data>,
    Path(guild_id): Path<GuildId>,
    AxumJson(request): AxumJson<DjChannelStatusRequest<ChannelStatus>>,
) -> Result<StatusCode, ApiError> {
    let status = request.timed(|status| status)?;
    send_dj_command(
        &bot_state,
        guild_id,
        DJCommand::SetOverrideChannelStatus(status),
    )
}
