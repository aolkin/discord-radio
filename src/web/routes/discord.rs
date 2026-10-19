use crate::bot_profile::ProfileEdit;
use crate::state::Data;
use crate::web::error::{ApiError, WithStatus};
use anyhow::Context;
use axum::Json as AxumJson;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use serenity::model::id::{ChannelId, GuildId};

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
        .send_message(crate::web::http(), create_message)
        .await
        .map_err(discord_error)?;

    Ok(StatusCode::OK)
}

pub async fn get_bot_profile(
    State(bot_state): State<Data>,
    Path(guild_id): Path<GuildId>,
) -> Result<AxumJson<serde_json::Value>, ApiError> {
    let member = crate::web::http()
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

    crate::web::http()
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
