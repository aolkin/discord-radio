use crate::audio::dj::manager::{
    ChannelStatusRequest, DJCommand, DjBusy, HexMessageRequest, IdleRequest, ProfileNotFound,
    SignalProfileRequest, TrackRequest, ensure_pool,
};
use crate::state::Data;
use crate::web::error::ApiError;
use axum::Json as AxumJson;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::Deserialize;
use serenity::model::id::GuildId;

#[derive(Debug, Deserialize)]
pub struct AdvanceDjStateRequest {
    pub state_type: Option<String>,
}

pub async fn advance_dj_state(
    State(bot_state): State<Data>,
    Path(guild_id): Path<GuildId>,
    AxumJson(request): AxumJson<AdvanceDjStateRequest>,
) -> Result<StatusCode, ApiError> {
    ensure_pool(bot_state, request.state_type.as_deref()).map_err(invalid_request)?;
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

fn invalid_request(error: anyhow::Error) -> ApiError {
    let status = if error.is::<ProfileNotFound>() {
        StatusCode::NOT_FOUND
    } else {
        StatusCode::BAD_REQUEST
    };
    ApiError::new(status, error)
}

pub async fn play_hex_message(
    State(bot_state): State<Data>,
    Path(guild_id): Path<GuildId>,
    AxumJson(request): AxumJson<HexMessageRequest>,
) -> Result<StatusCode, ApiError> {
    request.validate(bot_state).map_err(invalid_request)?;
    send_dj_command(&bot_state, guild_id, DJCommand::ForceHexMessage(request))
}

pub async fn play_track(
    State(bot_state): State<Data>,
    Path(guild_id): Path<GuildId>,
    AxumJson(request): AxumJson<TrackRequest>,
) -> Result<StatusCode, ApiError> {
    request.validate().map_err(invalid_request)?;
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
    request.validate(bot_state).map_err(invalid_request)?;
    send_dj_command(&bot_state, guild_id, DJCommand::ForceIdle(request))
}

pub async fn set_dj_profile(
    State(bot_state): State<Data>,
    Path(guild_id): Path<GuildId>,
    AxumJson(request): AxumJson<SignalProfileRequest>,
) -> Result<StatusCode, ApiError> {
    let command = request.into_command(bot_state).map_err(invalid_request)?;
    send_dj_command(&bot_state, guild_id, command)
}

pub async fn set_dj_channel_status(
    State(bot_state): State<Data>,
    Path(guild_id): Path<GuildId>,
    AxumJson(request): AxumJson<ChannelStatusRequest>,
) -> Result<StatusCode, ApiError> {
    let command = request.into_command().map_err(invalid_request)?;
    send_dj_command(&bot_state, guild_id, command)
}
