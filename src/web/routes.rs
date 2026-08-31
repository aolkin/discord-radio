use crate::state::Data;
use crate::web::state_snapshot::BotSnapshot;
use axum::Json as AxumJson;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Json};
use serde::{Deserialize, Serialize};
use serenity::model::id::GuildId;

pub async fn index() -> Html<&'static str> {
    Html(include_str!("../../static/index.html"))
}

pub async fn logs() -> Html<&'static str> {
    Html(include_str!("../../static/logs.html"))
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
    Path(guild_id): Path<String>,
    AxumJson(request): AxumJson<AdvanceDjStateRequest>,
) -> Result<StatusCode, StatusCode> {
    let guild_id_parsed: u64 = guild_id.parse().map_err(|_| StatusCode::BAD_REQUEST)?;
    let guild_id = GuildId::new(guild_id_parsed);

    let dj_managers = bot_state.dj_managers.read().await;
    let manager = match dj_managers.get(&guild_id) {
        Some(mgr) => mgr.clone(),
        None => return Err(StatusCode::NOT_FOUND),
    };
    drop(dj_managers);

    let mgr = manager.lock().await;
    if !mgr.is_running() {
        return Err(StatusCode::CONFLICT);
    }
    drop(mgr);

    let state_type_filter = if let Some(type_str) = request.state_type {
        match type_str.to_lowercase().as_str() {
            "track" => Some(crate::audio::dj::manager::DJStateTypeFilter::Track),
            "hex" | "hex_message" | "hexmessage" => {
                Some(crate::audio::dj::manager::DJStateTypeFilter::HexMessage)
            }
            "noise" => Some(crate::audio::dj::manager::DJStateTypeFilter::Noise),
            _ => return Err(StatusCode::BAD_REQUEST),
        }
    } else {
        None
    };

    crate::audio::dj::manager::force_advance(&bot_state, guild_id, state_type_filter)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(StatusCode::OK)
}

#[derive(Debug, Deserialize)]
pub struct PlayHexMessageRequest {
    pub message: String,
}

pub async fn play_hex_message(
    State(bot_state): State<Data>,
    Path(guild_id): Path<String>,
    AxumJson(request): AxumJson<PlayHexMessageRequest>,
) -> Result<StatusCode, StatusCode> {
    let guild_id_parsed: u64 = guild_id.parse().map_err(|_| StatusCode::BAD_REQUEST)?;
    let guild_id = GuildId::new(guild_id_parsed);

    crate::audio::dj::manager::force_hex_message(&bot_state, guild_id, request.message)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(StatusCode::OK)
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
    Path(guild_id): Path<String>,
    AxumJson(request): AxumJson<ChangeTrackStateRequest>,
) -> Result<StatusCode, StatusCode> {
    let guild_id_parsed: u64 = guild_id.parse().map_err(|_| StatusCode::BAD_REQUEST)?;
    let guild_id = GuildId::new(guild_id_parsed);

    let manager_arc = crate::audio::tracks::get_or_create_track_manager(&bot_state, guild_id).await;
    let mut manager = manager_arc.lock().await;
    let fade_time = request.fade_time.unwrap_or(1.0);

    let state_str = request.state.as_deref().map(|s| s.to_lowercase());
    match state_str.as_deref() {
        Some("start") => {
            let Some(filename) = request.filename else {
                return Err(StatusCode::BAD_REQUEST);
            };

            // Resolve here purely to validate the filename before touching any
            // existing track; `start_track` resolves it again to actually play it.
            bot_state
                .file_resolver
                .resolve(&filename)
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            let volume = request.volume.unwrap_or(1.0);
            let loops = request.loops.unwrap_or(true);

            if manager.has_track(&request.name) {
                let manager_clone = manager_arc.clone();
                let name_clone = request.name.clone();
                tokio::spawn(async move {
                    let mut mgr = manager_clone.lock().await;
                    let _ = mgr.stop_track(&name_clone, fade_time, false).await;
                });
            }

            manager
                .start_track(crate::audio::tracks::StartTrackArgs {
                    name: request.name,
                    filename,
                    volume,
                    fade_time,
                    loops,
                    start_position: None,
                    persist: true,
                })
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        }
        Some("stop") => {
            manager
                .stop_track(&request.name, fade_time, true)
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        }
        None => {
            if !manager.has_track(&request.name) {
                return Err(StatusCode::NOT_FOUND);
            }

            if let Some(new_volume) = request.volume {
                manager
                    .update_track_volume(&request.name, new_volume, fade_time)
                    .await
                    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            }

            if let Some(new_loops) = request.loops {
                manager
                    .update_track_loops(&request.name, new_loops)
                    .await
                    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            }
        }
        Some(_) => return Err(StatusCode::BAD_REQUEST),
    }

    Ok(StatusCode::OK)
}

#[derive(Debug, Deserialize)]
pub struct SignalProfileRequest {
    pub profile: String,
    pub fade_duration: Option<f32>,
}

pub async fn signal_profile(
    State(bot_state): State<Data>,
    Path(guild_id): Path<String>,
    AxumJson(request): AxumJson<SignalProfileRequest>,
) -> Result<StatusCode, StatusCode> {
    let guild_id_parsed: u64 = guild_id.parse().map_err(|_| StatusCode::BAD_REQUEST)?;
    let guild_id = GuildId::new(guild_id_parsed);

    let fade_duration_ms = request.fade_duration.unwrap_or(2.0) * 1000.0;

    if fade_duration_ms < 0.0 {
        return Err(StatusCode::BAD_REQUEST);
    }

    let processors = bot_state.audio_processors.read().await;
    let processor_arc = processors.get(&guild_id).cloned();
    drop(processors);

    let Some(processor_arc) = processor_arc else {
        return Err(StatusCode::NOT_FOUND);
    };

    if request.profile == "bypass" {
        let mut processor = processor_arc.write().await;
        processor.set_bypass(true);
        drop(processor);

        let profile_state = crate::persistence::ProfileState {
            profile_name: "bypass".to_string(),
            bypass: true,
        };
        let _ = bot_state
            .state_store
            .save_profile_state(guild_id, &profile_state)
            .await;
    } else {
        let Some(new_profile) = bot_state.profile_manager.get_profile(&request.profile) else {
            return Err(StatusCode::NOT_FOUND);
        };

        let mut processor = processor_arc.write().await;
        if fade_duration_ms > 0.0 {
            processor.start_profile_transition(new_profile.clone(), fade_duration_ms);
        } else {
            processor.set_profile_immediate(new_profile.clone());
        }
        drop(processor);

        let profile_state = crate::persistence::ProfileState {
            profile_name: request.profile,
            bypass: false,
        };
        let _ = bot_state
            .state_store
            .save_profile_state(guild_id, &profile_state)
            .await;
    }

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

#[derive(Debug, Serialize)]
pub struct AudioFilesResponse {
    pub files: Vec<String>,
}

pub async fn get_audio_files(State(bot_state): State<Data>) -> AxumJson<AudioFilesResponse> {
    let content_path = &bot_state.content_path;
    let mut files = Vec::new();

    if let Ok(entries) = std::fs::read_dir(content_path) {
        for entry in entries.flatten() {
            if let Ok(file_type) = entry.file_type()
                && file_type.is_file()
                && let Some(name) = entry.file_name().to_str()
                && (name.ends_with(".mp3") || name.ends_with(".wav") || name.ends_with(".ogg"))
            {
                files.push(name.to_string());
            }
        }
    }

    files.sort();
    AxumJson(AudioFilesResponse { files })
}

// Helper function to parse activity type from string
fn parse_activity_type(type_str: &str) -> Result<crate::voice_status::ActivityType, StatusCode> {
    match type_str.to_lowercase().as_str() {
        "listening" => Ok(crate::voice_status::ActivityType::Listening),
        "playing" => Ok(crate::voice_status::ActivityType::Playing),
        "streaming" => Ok(crate::voice_status::ActivityType::Streaming),
        "custom" => Ok(crate::voice_status::ActivityType::Custom),
        _ => Err(StatusCode::BAD_REQUEST),
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
) -> Result<StatusCode, StatusCode> {
    let activity_type = parse_activity_type(&request.activity_type)?;

    bot_state
        .activity_manager
        .set_activity(activity_type, request.status)
        .await;

    Ok(StatusCode::OK)
}

pub async fn push_bot_activity(
    State(bot_state): State<Data>,
    AxumJson(request): AxumJson<ActivityRequest>,
) -> Result<StatusCode, StatusCode> {
    let activity_type = parse_activity_type(&request.activity_type)?;

    bot_state
        .activity_manager
        .push_activity(activity_type, request.status)
        .await;

    Ok(StatusCode::OK)
}

pub async fn remove_bot_activity(
    State(bot_state): State<Data>,
    AxumJson(request): AxumJson<ActivityRequest>,
) -> Result<StatusCode, StatusCode> {
    let activity_type = parse_activity_type(&request.activity_type)?;

    let was_removed = bot_state
        .activity_manager
        .remove_activity(activity_type, &request.status)
        .await;

    if was_removed {
        Ok(StatusCode::OK)
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}

// DJ Config Override management endpoints

async fn trigger_dj_config_reload(bot_state: &Data) {
    // Send reload command to all running DJs
    let dj_managers = bot_state.dj_managers.read().await;
    for (guild_id, manager_arc) in dj_managers.iter() {
        let manager = manager_arc.lock().await;
        if let Some(tx) = &manager.command_tx {
            if let Err(e) = tx
                .send(crate::audio::dj::manager::DJCommand::ReloadConfig)
                .await
            {
                tracing::warn!(
                    "Failed to send reload command to DJ in guild {}: {}",
                    guild_id,
                    e
                );
            } else {
                tracing::debug!("Sent config reload command to DJ in guild {}", guild_id);
            }
        }
    }
}

#[derive(Debug, Serialize)]
pub struct DJConfigOverridesResponse {
    pub hex_messages: DJConfigOverrideCategoryResponse,
    pub hex_message_announcements: DJConfigOverrideCategoryResponse,
    pub state_weights: DJConfigOverrideSingleResponse,
}

#[derive(Debug, Serialize)]
pub struct DJConfigOverrideCategoryResponse {
    pub enabled: bool,
    pub items: serde_json::Value,
}

#[derive(Debug, Serialize)]
pub struct DJConfigOverrideSingleResponse {
    pub enabled: bool,
    pub value: serde_json::Value,
}

pub async fn get_dj_config_overrides(
    State(bot_state): State<Data>,
) -> AxumJson<DJConfigOverridesResponse> {
    let overrides_arc = bot_state.dj_config_overrides.get_arc();
    let overrides = overrides_arc.read().await;

    let hex_messages_items =
        serde_json::to_value(&overrides.hex_messages.items).unwrap_or(serde_json::Value::Null);
    let hex_message_announcements_items =
        serde_json::to_value(&overrides.hex_message_announcements.items)
            .unwrap_or(serde_json::Value::Null);
    let state_weights_value =
        serde_json::to_value(&overrides.state_weights.value).unwrap_or(serde_json::Value::Null);

    AxumJson(DJConfigOverridesResponse {
        hex_messages: DJConfigOverrideCategoryResponse {
            enabled: overrides.hex_messages.enabled,
            items: hex_messages_items,
        },
        hex_message_announcements: DJConfigOverrideCategoryResponse {
            enabled: overrides.hex_message_announcements.enabled,
            items: hex_message_announcements_items,
        },
        state_weights: DJConfigOverrideSingleResponse {
            enabled: overrides.state_weights.enabled,
            value: state_weights_value,
        },
    })
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
    AxumJson(request): AxumJson<SetHexMessageRequest>,
) -> Result<StatusCode, StatusCode> {
    use crate::audio::dj::config::HexMessageEntry;

    if request.text.trim().is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    let hex_message = HexMessageEntry {
        text: request.text,
        weight: request.weight,
        signal_profile: request.signal_profile,
        loop_min: request.loop_min,
        loop_max: request.loop_max,
        announcement: request.announcement,
    };

    bot_state
        .dj_config_overrides
        .set_hex_message(request.index, hex_message)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Trigger config reload for all running DJs
    trigger_dj_config_reload(&bot_state).await;

    Ok(StatusCode::OK)
}

pub async fn delete_hex_message_override(
    State(bot_state): State<Data>,
    Path(index): Path<usize>,
) -> Result<StatusCode, StatusCode> {
    bot_state
        .dj_config_overrides
        .delete_hex_message(index)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;

    // Trigger config reload for all running DJs
    trigger_dj_config_reload(&bot_state).await;

    Ok(StatusCode::OK)
}

#[derive(Debug, Deserialize)]
pub struct SetAnnouncementRequest {
    pub index: Option<usize>,
    pub text: String,
}

pub async fn set_announcement_override(
    State(bot_state): State<Data>,
    AxumJson(request): AxumJson<SetAnnouncementRequest>,
) -> Result<StatusCode, StatusCode> {
    if request.text.trim().is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    bot_state
        .dj_config_overrides
        .set_announcement(request.index, request.text)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Trigger config reload for all running DJs
    trigger_dj_config_reload(&bot_state).await;

    Ok(StatusCode::OK)
}

pub async fn delete_announcement_override(
    State(bot_state): State<Data>,
    Path(index): Path<usize>,
) -> Result<StatusCode, StatusCode> {
    bot_state
        .dj_config_overrides
        .delete_announcement(index)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;

    // Trigger config reload for all running DJs
    trigger_dj_config_reload(&bot_state).await;

    Ok(StatusCode::OK)
}

#[derive(Debug, Deserialize)]
pub struct ToggleOverrideCategoryRequest {
    pub category: String,
    pub enabled: bool,
}

pub async fn toggle_override_category(
    State(bot_state): State<Data>,
    AxumJson(request): AxumJson<ToggleOverrideCategoryRequest>,
) -> Result<StatusCode, StatusCode> {
    bot_state
        .dj_config_overrides
        .toggle_category(&request.category, request.enabled)
        .await
        .map_err(|_| StatusCode::BAD_REQUEST)?;

    // Trigger config reload for all running DJs
    trigger_dj_config_reload(&bot_state).await;

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
    AxumJson(request): AxumJson<SetStateWeightsRequest>,
) -> Result<StatusCode, StatusCode> {
    use crate::audio::dj::config::StateWeights;

    let weights = StateWeights {
        track: request.track,
        hex_message: request.hex_message,
        noise: request.noise,
    };

    bot_state
        .dj_config_overrides
        .set_state_weights(weights)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Trigger config reload for all running DJs
    trigger_dj_config_reload(&bot_state).await;

    Ok(StatusCode::OK)
}

pub async fn get_default_state_weights(
    State(bot_state): State<Data>,
) -> Result<AxumJson<crate::audio::dj::config::StateWeights>, StatusCode> {
    use crate::audio::dj::config::DJConfig;

    // Load the default DJ config
    let config_path = format!("{}/dj_configs/default.json", bot_state.content_path);
    let config =
        DJConfig::load_from_file(&config_path).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(AxumJson(config.state_weights))
}

// Log retrieval endpoints

use crate::logging::{LogReader, guild_logs_dir};
use axum::extract::Query;

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
    Path((guild_id, log_type)): Path<(String, String)>,
    Query(params): Query<LogQueryParams>,
) -> Result<AxumJson<crate::logging::LogReadResult>, StatusCode> {
    // Parse guild_id
    let guild_id_parsed: u64 = guild_id.parse().map_err(|_| StatusCode::BAD_REQUEST)?;

    // Validate log type (only allow specific log types for security)
    let log_filename = match log_type.as_str() {
        "members" => "members.jsonl",
        "dj" => "dj.jsonl",
        _ => return Err(StatusCode::BAD_REQUEST),
    };

    // Build the log file path
    let log_path = guild_logs_dir(&bot_state.logs_base_path, guild_id_parsed).join(log_filename);

    // Create the log reader
    let reader = LogReader::new(log_path.clone());

    // Validate that the path is within the logs directory (security check)
    let logs_dir = crate::logging::logs_dir(&bot_state.logs_base_path);
    if reader.validate_path(&logs_dir).is_err() {
        tracing::error!("Attempt to read log outside logs directory: {:?}", log_path);
        return Err(StatusCode::FORBIDDEN);
    }

    // Read the logs based on parameters
    let result = match (params.offset, params.direction.as_str()) {
        (None, "backward") | (None, _) => {
            // Tail behavior - get the last n entries
            reader.tail(params.limit).await
        }
        (Some(offset), "backward") => {
            // Read n entries before the offset
            reader.read_before(offset, params.limit).await
        }
        (Some(offset), "forward") => {
            // Read n entries after the offset
            reader.read_after(offset, params.limit).await
        }
        _ => return Err(StatusCode::BAD_REQUEST),
    };

    match result {
        Ok(log_result) => Ok(AxumJson(log_result)),
        Err(e) => {
            tracing::error!("Failed to read logs: {:?}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
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
) -> Result<AxumJson<RegisteredChannelsResponse>, StatusCode> {
    let channels = bot_state
        .state_store
        .load_registered_channels()
        .await
        .map_err(|e| {
            tracing::error!("Failed to load registered channels: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

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
    Path(channel_id): Path<String>,
    AxumJson(request): AxumJson<SendChannelMessageRequest>,
) -> Result<StatusCode, StatusCode> {
    let channel_id_parsed: u64 = channel_id.parse().map_err(|_| StatusCode::BAD_REQUEST)?;
    let channel_id = serenity::model::id::ChannelId::new(channel_id_parsed);

    // Verify the channel ID is in the list of registered channels
    let registered_channels = bot_state
        .state_store
        .load_registered_channels()
        .await
        .map_err(|e| {
            tracing::error!("Failed to load registered channels: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let registered_channel = registered_channels
        .iter()
        .find(|c| c.channel_id == channel_id)
        .ok_or_else(|| {
            tracing::warn!(
                "Attempt to send message to unregistered channel {}",
                channel_id
            );
            StatusCode::FORBIDDEN
        })?;

    // Get serenity HTTP client from activity manager context
    let ctx = bot_state.activity_manager.get_context().await;
    let Some(ctx) = ctx else {
        tracing::error!("Bot context not available for sending messages");
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    };

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
        .send_message(&ctx.http, create_message)
        .await
        .map_err(|e| {
            tracing::error!(
                "Failed to send message to channel {} in guild {}: {}",
                channel_id,
                registered_channel.guild_id,
                e
            );
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok(StatusCode::OK)
}
