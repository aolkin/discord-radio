use crate::bot_profile::ProfileEdit;
use crate::state::Data;
use crate::web::error::{ApiError, WithStatus};
use axum::Json as AxumJson;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use serenity::model::channel::ChannelType;
use serenity::model::id::{ChannelId, GuildId};
use serenity::model::permissions::Permissions;

#[derive(Debug, Serialize)]
pub struct GuildResponse {
    pub guild_id: String,
    pub name: String,
}

#[derive(Debug, Serialize)]
pub struct GuildsResponse {
    pub guilds: Vec<GuildResponse>,
}

pub async fn get_guilds(
    State(bot_state): State<Data>,
) -> Result<AxumJson<GuildsResponse>, ApiError> {
    let cache = ready_cache(&bot_state)?;
    let mut guilds: Vec<_> = cache
        .guilds()
        .into_iter()
        .filter_map(|id| cache.guild(id).map(|g| (id, g.name.clone())))
        .collect();
    guilds.sort_by(|a, b| {
        a.1.to_lowercase()
            .cmp(&b.1.to_lowercase())
            .then(a.0.cmp(&b.0))
    });

    Ok(AxumJson(GuildsResponse {
        guilds: guilds
            .into_iter()
            .map(|(id, name)| GuildResponse {
                guild_id: id.to_string(),
                name,
            })
            .collect(),
    }))
}

#[derive(Debug, Serialize)]
pub struct SendableChannelResponse {
    pub channel_id: String,
    pub name: String,
}

#[derive(Debug, Serialize)]
pub struct SendableChannelsResponse {
    pub channels: Vec<SendableChannelResponse>,
}

fn ready_cache(bot_state: &Data) -> Result<&std::sync::Arc<serenity::cache::Cache>, ApiError> {
    bot_state.cache.get().ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            anyhow::anyhow!("The Discord cache is not ready yet"),
        )
    })
}

pub fn sendable_channels(
    bot_state: Data,
    guild_id: GuildId,
) -> Result<Vec<(ChannelId, String)>, ApiError> {
    let cache = ready_cache(&bot_state)?;
    let guild = cache
        .guild(guild_id)
        .ok_or_else(|| ApiError::not_found(format!("Guild {guild_id} is not known to the bot")))?;
    let member = guild.members.get(&bot_state.bot_user_id).ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            anyhow::anyhow!("The bot member of guild {guild_id} is not cached"),
        )
    })?;

    let required = Permissions::VIEW_CHANNEL | Permissions::SEND_MESSAGES;
    let mut channels: Vec<_> = guild
        .channels
        .values()
        .filter(|c| c.kind == ChannelType::Text)
        .filter(|c| guild.user_permissions_in(c, member).contains(required))
        .collect();
    channels.sort_by_key(|c| (c.position, c.id));

    Ok(channels
        .into_iter()
        .map(|c| (c.id, c.name.clone()))
        .collect())
}

pub async fn get_sendable_channels(
    State(bot_state): State<Data>,
    Path(guild_id): Path<GuildId>,
) -> Result<AxumJson<SendableChannelsResponse>, ApiError> {
    let channels = sendable_channels(bot_state, guild_id)?
        .into_iter()
        .map(|(id, name)| SendableChannelResponse {
            channel_id: id.to_string(),
            name,
        })
        .collect();

    Ok(AxumJson(SendableChannelsResponse { channels }))
}

#[derive(Debug, Deserialize)]
pub struct SendChannelMessageRequest {
    pub message: Option<String>,
    pub title: Option<String>,
    pub description: Option<String>,
}

pub async fn send_channel_message(
    State(bot_state): State<Data>,
    Path((guild_id, channel_id)): Path<(GuildId, ChannelId)>,
    AxumJson(request): AxumJson<SendChannelMessageRequest>,
) -> Result<StatusCode, ApiError> {
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

    let create_message = crate::commands::message::build_message(
        request.message,
        request.title,
        request.description,
    );

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
