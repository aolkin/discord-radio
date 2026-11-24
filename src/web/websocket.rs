use crate::audio::dj::manager::SignalPresetRequest;
use crate::audio::dj::signal_preset::DEFAULT_PRESET_FADE;
use crate::audio::dj::tiered::{ConfigurableTier, Tier};
use crate::audio::presets::{EffectEntry, SignalParameters};
use crate::commands::dj::send_to_dj;
use crate::state::{Data, GuildRadio};
use crate::web::state_snapshot::BotSnapshot;
use anyhow::{Context, bail};
use axum::extract::ws::{Message, WebSocket};
use axum::extract::{State, WebSocketUpgrade};
use axum::response::Response;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use serenity::model::id::GuildId;
use tokio::sync::broadcast::error::RecvError;
use tokio::time::{Duration, interval};
use tracing::Instrument;

#[derive(Debug, Clone)]
pub struct SignalEdit {
    pub guild_id: GuildId,
    pub parameters: SignalParameters,
}

#[derive(Debug, Deserialize)]
struct ClientMessage {
    guild_id: GuildId,
    #[serde(flatten)]
    action: ClientAction,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum ClientAction {
    #[serde(rename = "start_live_signal")]
    Start,
    #[serde(rename = "add_live_effect")]
    AddEffect { index: usize, effect: EffectEntry },
    #[serde(rename = "edit_live_effect")]
    EditEffect {
        name: String,
        params: Map<String, Value>,
    },
    #[serde(rename = "remove_live_effect")]
    RemoveEffect { name: String },
    #[serde(rename = "move_live_effect")]
    MoveEffect { name: String, index: usize },
    #[serde(rename = "load_live_signal")]
    Load { signal_preset: String },
    #[serde(rename = "end_live_signal")]
    End,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ServerMessage {
    Snapshot(BotSnapshot),
    SignalParameters {
        guild_id: String,
        parameters: SignalParameters,
    },
    Error {
        guild_id: Option<String>,
        message: String,
    },
}

pub async fn websocket_handler(ws: WebSocketUpgrade, State(bot_state): State<Data>) -> Response {
    let span = tracing::Span::current();
    ws.on_upgrade(|socket| handle_socket(socket, bot_state).instrument(span))
}

async fn handle_socket(mut socket: WebSocket, bot_state: Data) {
    let mut edits = bot_state.signal_edits.subscribe();
    let mut interval = interval(Duration::from_secs(1));

    loop {
        let message = tokio::select! {
            _ = interval.tick() => ServerMessage::Snapshot(BotSnapshot::capture(&bot_state).await),
            edit = edits.recv() => match edit {
                Ok(edit) => ServerMessage::SignalParameters {
                    guild_id: edit.guild_id.to_string(),
                    parameters: edit.parameters,
                },
                Err(RecvError::Lagged(skipped)) => {
                    tracing::warn!("WebSocket client missed {skipped} signal edits");
                    continue;
                }
                Err(RecvError::Closed) => break,
            },
            received = socket.recv() => match received {
                Some(Ok(Message::Text(text))) => {
                    match handle_message(bot_state, &text).await {
                        Ok(()) => continue,
                        Err((guild_id, e)) => ServerMessage::Error {
                            guild_id: guild_id.map(|id| id.to_string()),
                            message: format!("{e:#}"),
                        },
                    }
                }
                Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                Some(Ok(_)) => continue,
            },
        };

        let json = match serde_json::to_string(&message) {
            Ok(j) => j,
            Err(e) => {
                tracing::error!("Failed to serialize WebSocket message: {e:#}");
                continue;
            }
        };

        if socket.send(Message::Text(json.into())).await.is_err() {
            break;
        }
    }
    tracing::debug!("WebSocket client disconnected");
}

async fn handle_message(
    bot_state: Data,
    text: &str,
) -> Result<(), (Option<GuildId>, anyhow::Error)> {
    let ClientMessage { guild_id, action } = serde_json::from_str(text)
        .context("reading WebSocket message")
        .map_err(|e| (None, e))?;
    bot_state.touch(guild_id);
    handle_action(bot_state, guild_id, action)
        .await
        .map_err(|e| (Some(guild_id), e))
}

async fn handle_action(
    bot_state: Data,
    guild_id: GuildId,
    action: ClientAction,
) -> anyhow::Result<()> {
    let radio = bot_state.require_radio(guild_id)?;
    match action {
        ClientAction::Start => {
            let active = radio.dj_manager.state().read().await.signal_preset();
            if active.tier != Tier::Override {
                set_override(bot_state, guild_id, Some(active.value.name))?;
            }
        }
        ClientAction::AddEffect { index, effect } => {
            edit_live(bot_state, guild_id, &radio, |preset| {
                preset.with_effect_added(index, effect)
            })
            .await?
        }
        ClientAction::EditEffect { name, params } => {
            edit_live(bot_state, guild_id, &radio, |preset| {
                preset.with_params_edited(&name, params)
            })
            .await?
        }
        ClientAction::RemoveEffect { name } => {
            edit_live(bot_state, guild_id, &radio, |preset| {
                preset.without_effect(&name)
            })
            .await?
        }
        ClientAction::MoveEffect { name, index } => {
            edit_live(bot_state, guild_id, &radio, |preset| {
                preset.with_effect_moved(&name, index)
            })
            .await?
        }
        ClientAction::Load { signal_preset } => {
            let preset = bot_state.preset_manager.require(&signal_preset)?;
            set_override(bot_state, guild_id, Some(signal_preset))?;
            radio.audio_processor.write().await.start_preset_transition(
                (*preset).clone(),
                DEFAULT_PRESET_FADE.as_secs_f32() * 1000.0,
            );
            send_edit(bot_state, guild_id, (*preset).clone());
        }
        ClientAction::End => set_override(bot_state, guild_id, None)?,
    }
    Ok(())
}

async fn edit_live(
    bot_state: Data,
    guild_id: GuildId,
    radio: &GuildRadio,
    edit: impl FnOnce(&SignalParameters) -> anyhow::Result<SignalParameters>,
) -> anyhow::Result<()> {
    if radio.dj_manager.state().read().await.signal_preset().tier != Tier::Override {
        bail!("no live signal edit is active: the signal preset override is not set");
    }
    let mut processor = radio.audio_processor.write().await;
    let edited = edit(processor.target_preset())?;
    processor.set_preset_immediate(edited.clone());
    drop(processor);
    send_edit(bot_state, guild_id, edited);
    Ok(())
}

fn set_override(bot_state: Data, guild_id: GuildId, preset: Option<String>) -> anyhow::Result<()> {
    let request = SignalPresetRequest {
        tier: ConfigurableTier::Override,
        preset,
        fade_duration: None,
        duration: None,
    };
    send_to_dj(bot_state, guild_id, request.into_command(bot_state)?)
        .context("updating the signal preset override")
}

fn send_edit(bot_state: Data, guild_id: GuildId, parameters: SignalParameters) {
    let _ = bot_state.signal_edits.send(SignalEdit {
        guild_id,
        parameters,
    });
}
