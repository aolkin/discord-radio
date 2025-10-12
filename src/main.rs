mod audio;
mod commands;
mod handlers;
mod persistence;
mod shutdown;
mod startup;
mod state;

use crate::state::BotState;
use poise::serenity_prelude::{self as serenity, GatewayIntents};
use songbird::SerenityInit;
use std::sync::Arc;
use tracing::info;

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
    async fn ready(&self, ctx: serenity::Context, ready: serenity::Ready) {
        tracing::info!("{} is connected!", ready.user.name);

        if let Err(e) = crate::startup::restore_voice_channels(&ctx, self.data.clone()).await {
            tracing::error!("Failed to restore voice channels: {:?}", e);
        }
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
    // Set up panic handler to crash the whole process on any thread panic
    // This ensures the OS can restart the bot instead of running in a broken state
    let default_panic = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        default_panic(panic_info);
        tracing::error!("Thread panicked, exiting process: {:?}", panic_info);
        std::process::exit(1);
    }));

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                tracing_subscriber::EnvFilter::new("info")
                    .add_directive("songbird=warn".parse().unwrap())
                    .add_directive("symphonia_core=warn".parse().unwrap())
                    .add_directive("symphonia_format_ogg=warn".parse().unwrap())
            }),
        )
        .init();

    dotenvy::dotenv().ok();

    let discord_token =
        std::env::var("DISCORD_TOKEN").expect("Expected DISCORD_TOKEN in the environment");

    let args: Vec<String> = std::env::args().collect();
    let content_path = args
        .get(1)
        .expect("Usage: discord-bot <content_path>")
        .clone();

    tracing::info!("Using content path: {}", content_path);

    let state_store_path = std::path::PathBuf::from(
        std::env::var("STATE_STORE_PATH").unwrap_or_else(|_| "./bot-state".to_string()),
    );
    tracing::info!("Using state store path: {:?}", state_store_path);

    let state_store = Arc::new(persistence::FileStore::new(state_store_path));

    let data = Arc::new(BotState::new(content_path, state_store));

    let data_for_setup = data.clone();
    let framework = poise::Framework::builder()
        .options(poise::FrameworkOptions {
            commands: vec![
                commands::voice::join_voice_channel(),
                commands::voice::leave_voice_channel(),
                commands::voice::play_message(),
                commands::voice::stop_message(),
                commands::voice::change_track_state(),
                commands::voice::get_current_tracks(),
                commands::voice::signal_profile(),
                commands::voice::manage_dj(),
                commands::voice::get_dj_state(),
                commands::voice::advance_dj_state(),
                commands::messaging::speak(),
                commands::messaging::set_status(),
            ],
            on_error: |error| Box::pin(on_error(error)),
            ..Default::default()
        })
        .setup(|ctx, _ready, framework| {
            Box::pin(async move {
                poise::builtins::register_globally(ctx, &framework.options().commands).await?;
                let command_names: Vec<_> = framework
                    .options()
                    .commands
                    .iter()
                    .map(|cmd| cmd.name.as_str())
                    .collect();
                info!(
                    "Registered {} commands globally: {}",
                    framework.options().commands.len(),
                    command_names.join(", ")
                );
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

    // Restore saved state (excluding voice channels which need Context)
    tracing::info!("Restoring saved state...");
    let http_for_restore = client.http.clone();
    if let Err(e) = startup::restore_state(http_for_restore, data.clone()).await {
        tracing::error!("Failed to restore state: {:?}", e);
    }

    // Set up graceful shutdown handling
    shutdown::setup_shutdown_handler(data.clone()).await;

    client.start().await?;

    Ok(())
}
