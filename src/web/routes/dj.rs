use crate::audio::dj::manager::{
    ChannelStatusRequest, ConfigureRequest, DJCommand, DjBusy, IdleRequest, PresetNotFound,
    SignalPresetRequest, TrackRequest, TtsMessageRequest, ensure_pool,
};
use crate::state::{Data, GuildInactive};
use crate::web::error::ApiError;
use crate::web::routes::discord::sendable_channels;
use axum::Json as AxumJson;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::Deserialize;
use serenity::model::id::{ChannelId, GuildId};

#[derive(Debug, Deserialize)]
pub struct AdvanceDjStateRequest {
    pub pool: Option<String>,
}

pub async fn advance_dj_state(
    State(bot_state): State<Data>,
    Path(guild_id): Path<GuildId>,
    AxumJson(request): AxumJson<AdvanceDjStateRequest>,
) -> Result<StatusCode, ApiError> {
    ensure_pool(bot_state, request.pool.as_deref()).map_err(invalid_request)?;
    send_dj_command(&bot_state, guild_id, DJCommand::ForceAdvance(request.pool))
}

fn send_dj_command(
    bot_state: &Data,
    guild_id: GuildId,
    command: DJCommand,
) -> Result<StatusCode, ApiError> {
    let sent = bot_state
        .require_radio(guild_id)
        .map_err(ApiError::inactive)?
        .dj_manager
        .send(command)
        .map_err(|e| {
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
        return Err(ApiError::inactive(GuildInactive(guild_id)));
    }
    Ok(StatusCode::OK)
}

fn invalid_request(error: anyhow::Error) -> ApiError {
    let status = if error.is::<PresetNotFound>() {
        StatusCode::NOT_FOUND
    } else {
        StatusCode::BAD_REQUEST
    };
    ApiError::new(status, error)
}

pub async fn play_tts_message(
    State(bot_state): State<Data>,
    Path(guild_id): Path<GuildId>,
    AxumJson(request): AxumJson<TtsMessageRequest>,
) -> Result<StatusCode, ApiError> {
    request.validate(bot_state).map_err(invalid_request)?;
    send_dj_command(&bot_state, guild_id, DJCommand::ForceTtsMessage(request))
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

pub async fn configure_dj(
    State(bot_state): State<Data>,
    Path(guild_id): Path<GuildId>,
    AxumJson(request): AxumJson<ConfigureRequest>,
) -> Result<StatusCode, ApiError> {
    let command = request.into_command(bot_state).map_err(invalid_request)?;
    send_dj_command(&bot_state, guild_id, command)
}

pub async fn set_dj_preset(
    State(bot_state): State<Data>,
    Path(guild_id): Path<GuildId>,
    AxumJson(request): AxumJson<SignalPresetRequest>,
) -> Result<StatusCode, ApiError> {
    let command = request.into_command(bot_state).map_err(invalid_request)?;
    send_dj_command(&bot_state, guild_id, command)
}

#[derive(Debug, Deserialize)]
pub struct AnnouncementChannelRequest {
    pub channel_id: ChannelId,
}

pub async fn add_announcement_channel(
    State(bot_state): State<Data>,
    Path(guild_id): Path<GuildId>,
    AxumJson(request): AxumJson<AnnouncementChannelRequest>,
) -> Result<StatusCode, ApiError> {
    let channel_id = request.channel_id;
    let sendable = sendable_channels(bot_state, guild_id)?
        .iter()
        .any(|(id, _)| *id == channel_id);
    if !sendable {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            anyhow::anyhow!(
                "The bot cannot send messages to channel {channel_id} in guild {guild_id}"
            ),
        ));
    }
    send_dj_command(
        &bot_state,
        guild_id,
        DJCommand::AddAnnouncementChannel(channel_id),
    )
}

pub async fn remove_announcement_channel(
    State(bot_state): State<Data>,
    Path((guild_id, channel_id)): Path<(GuildId, ChannelId)>,
) -> Result<StatusCode, ApiError> {
    let listed = bot_state
        .require_radio(guild_id)
        .map_err(ApiError::inactive)?
        .dj_manager
        .state()
        .read()
        .await
        .announcement_channels
        .contains(&channel_id);
    if !listed {
        return Err(ApiError::not_found(format!(
            "Channel {channel_id} is not an announcement channel in guild {guild_id}"
        )));
    }
    send_dj_command(
        &bot_state,
        guild_id,
        DJCommand::RemoveAnnouncementChannel(channel_id),
    )
}

pub async fn set_dj_channel_status(
    State(bot_state): State<Data>,
    Path(guild_id): Path<GuildId>,
    AxumJson(request): AxumJson<ChannelStatusRequest>,
) -> Result<StatusCode, ApiError> {
    let command = request.into_command().map_err(invalid_request)?;
    send_dj_command(&bot_state, guild_id, command)
}
