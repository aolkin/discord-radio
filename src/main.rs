mod audio;
mod commands;
mod handlers;
mod shutdown;
mod state;

use crate::state::BotState;
use poise::serenity_prelude::{self as serenity, GatewayIntents};
use songbird::SerenityInit;
use std::sync::Arc;

type Error = Box<dyn std::error::Error + Send + Sync>;

async fn on_error(error: poise::FrameworkError<'_, Arc<BotState>, Error>) {
    match error {
        poise::FrameworkError::Command { error, ctx, .. } => {
            tracing::error!("Error in command '{}': {:?}", ctx.command().name, error);
        }
        poise::FrameworkError::CommandCheckFailed { error, ctx, .. } => {
            tracing::error!(
                "Command check failed for '{}': {:?}",
                ctx.command().name,
                error
            );
        }
        poise::FrameworkError::ArgumentParse { error, ctx, .. } => {
            tracing::error!(
                "Failed to parse arguments for '{}': {}",
                ctx.command().name,
                error
            );
        }
        poise::FrameworkError::Setup { error, .. } => {
            tracing::error!("Setup error: {:?}", error);
        }
        poise::FrameworkError::EventHandler { error, event, .. } => {
            tracing::error!("Error in event handler for {:?}: {:?}", event, error);
        }
        error => {
            tracing::error!("Other framework error: {:?}", error);
        }
    }
}

struct Handler {
    data: Arc<BotState>,
}

#[serenity::async_trait]
impl serenity::EventHandler for Handler {
    async fn ready(&self, _: serenity::Context, ready: serenity::Ready) {
        tracing::info!("{} is connected!", ready.user.name);
    }

    async fn voice_state_update(
        &self,
        _ctx: serenity::Context,
        old: Option<serenity::VoiceState>,
        new: serenity::VoiceState,
    ) {
        // Only track voice state changes for channels we're connected to
        let guild_id = match new.guild_id {
            Some(id) => id,
            None => return,
        };

        let voice_connections = self.data.voice_connections.read().await;
        if !voice_connections.contains_key(&guild_id) {
            return;
        }

        let user_id = new.user_id;
        if user_id == _ctx.cache.current_user().id {
            return;
        }

        match (old.and_then(|o| o.channel_id), new.channel_id) {
            (None, Some(channel_id)) => {
                tracing::info!(
                    "User {} joined voice channel {} in guild {}",
                    user_id,
                    channel_id,
                    guild_id
                );
            }
            (Some(old_channel), None) => {
                tracing::info!(
                    "User {} left voice channel {} in guild {}",
                    user_id,
                    old_channel,
                    guild_id
                );
            }
            (Some(old_channel), Some(new_channel)) if old_channel != new_channel => {
                tracing::info!(
                    "User {} moved from voice channel {} to {} in guild {}",
                    user_id,
                    old_channel,
                    new_channel,
                    guild_id
                );
            }
            _ => {}
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing_subscriber::fmt::init();

    dotenvy::dotenv().ok();

    let discord_token =
        std::env::var("DISCORD_TOKEN").expect("Expected DISCORD_TOKEN in the environment");

    let args: Vec<String> = std::env::args().collect();
    let audio_file_path = args
        .get(1)
        .expect("Usage: discord-bot <audio_file_path>")
        .clone();

    tracing::info!("Using audio file: {}", audio_file_path);

    let data = Arc::new(BotState::new(audio_file_path));

    let data_for_setup = data.clone();
    let framework = poise::Framework::builder()
        .options(poise::FrameworkOptions {
            commands: vec![
                commands::admin::join_voice_channel(),
                commands::admin::leave_voice_channel(),
            ],
            on_error: |error| Box::pin(on_error(error)),
            ..Default::default()
        })
        .setup(|ctx, _ready, framework| {
            Box::pin(async move {
                poise::builtins::register_globally(ctx, &framework.options().commands).await?;
                Ok(data_for_setup.clone())
            })
        })
        .build();

    let intents = GatewayIntents::GUILDS | GatewayIntents::GUILD_VOICE_STATES;

    let mut client = serenity::Client::builder(&discord_token, intents)
        .framework(framework)
        .event_handler(Handler { data: data.clone() })
        .register_songbird()
        .await?;

    // Set up graceful shutdown handling
    shutdown::setup_shutdown_handler(data.clone()).await;

    client.start().await?;

    Ok(())
}
