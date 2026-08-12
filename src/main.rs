mod audio;
mod commands;
mod handlers;
mod logging;
mod metrics;
mod persistence;
mod shutdown;
mod startup;
mod state;
mod voice_status;
mod web;

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

        // Set the context for the activity manager
        self.data.activity_manager.set_context(ctx.clone()).await;

        // Restore activity state from disk
        if let Err(e) = self.data.activity_manager.restore_from_disk().await {
            tracing::error!("Failed to restore activity state: {:?}", e);
        }

        if let Err(e) = crate::startup::restore_voice_channels(&ctx, self.data.clone()).await {
            tracing::error!("Failed to restore voice channels: {:?}", e);
        }
    }

    async fn voice_state_update(
        &self,
        ctx: serenity::Context,
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
        drop(voice_connections);

        let user_id = new.user_id;
        if user_id == ctx.cache.current_user().id {
            return;
        }

        // Get user information
        let member = new.member.as_ref();
        let username = member.map(|m| m.user.name.clone()).unwrap_or_default();
        let nickname = member.and_then(|m| m.nick.clone());

        match (old.and_then(|o| o.channel_id), new.channel_id) {
            (None, Some(channel_id)) => {
                tracing::info!(
                    "User {} joined voice channel {} in guild {}",
                    user_id,
                    channel_id,
                    guild_id
                );

                // Log the join event
                if let Err(e) = self
                    .data
                    .log_member_activity(
                        guild_id.get(),
                        user_id.get(),
                        &username,
                        nickname.as_deref(),
                        "joined",
                        Some(channel_id.get()),
                    )
                    .await
                {
                    tracing::error!("Failed to log member join: {}", e);
                }
            }
            (Some(old_channel), None) => {
                tracing::info!(
                    "User {} left voice channel {} in guild {}",
                    user_id,
                    old_channel,
                    guild_id
                );

                // Log the leave event
                if let Err(e) = self
                    .data
                    .log_member_activity(
                        guild_id.get(),
                        user_id.get(),
                        &username,
                        nickname.as_deref(),
                        "left",
                        Some(old_channel.get()),
                    )
                    .await
                {
                    tracing::error!("Failed to log member leave: {}", e);
                }
            }
            (Some(old_channel), Some(new_channel)) if old_channel != new_channel => {
                tracing::info!(
                    "User {} moved from voice channel {} to {} in guild {}",
                    user_id,
                    old_channel,
                    new_channel,
                    guild_id
                );

                // Log the move event
                if let Err(e) = self
                    .data
                    .log_member_activity(
                        guild_id.get(),
                        user_id.get(),
                        &username,
                        nickname.as_deref(),
                        "moved",
                        Some(new_channel.get()),
                    )
                    .await
                {
                    tracing::error!("Failed to log member move: {}", e);
                }
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
                    .add_directive("discord_bot::audio::dj=debug".parse().unwrap())
                    .add_directive("discord_bot::audio::tracks=warn".parse().unwrap())
                    .add_directive("songbird=warn".parse().unwrap())
                    .add_directive("symphonia_core=warn".parse().unwrap())
                    .add_directive("symphonia_format_ogg=warn".parse().unwrap())
            }),
        )
        .init();

    dotenvy::dotenv().ok();

    let run_number = env!("BUILD_RUN_NUMBER");
    let commit_hash = env!("BUILD_COMMIT_HASH");

    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(|s| s.as_str()) == Some("--version") {
        println!("discord-bot-{} ({})", run_number, commit_hash);
        return Ok(());
    } else {
        info!("Starting discord-bot-{} ({})", run_number, commit_hash);
    }

    let discord_token =
        std::env::var("DISCORD_TOKEN").expect("Expected DISCORD_TOKEN in the environment");

    let web_port = std::env::var("WEB_PORT")
        .ok()
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(3000);

    let content_path = args
        .get(1)
        .expect("Usage: discord-bot <content_path>")
        .clone();

    tracing::info!("Using content path: {}", content_path);

    let state_store_path = std::path::PathBuf::from(
        std::env::var("STATE_STORE_PATH").unwrap_or_else(|_| "./bot-state".to_string()),
    );
    tracing::info!("Using state store path: {:?}", state_store_path);

    let state_store = Arc::new(persistence::FileStore::new(state_store_path.clone()));

    // Load DJ config overrides
    let dj_config_overrides_path = state_store_path.join("dj_config_overrides.json");
    let dj_config_overrides =
        match persistence::DJConfigOverrides::load_from_file(&dj_config_overrides_path) {
            Ok(overrides) => overrides,
            Err(_e) if !dj_config_overrides_path.exists() => {
                tracing::info!("DJ config overrides file does not exist, using defaults");
                persistence::DJConfigOverrides::default()
            }
            Err(e) => {
                tracing::error!("Failed to load DJ config overrides: {}", e);
                return Err(e);
            }
        };
    let dj_config_overrides_store =
        persistence::DJConfigOverridesStore::new(dj_config_overrides, dj_config_overrides_path);
    tracing::info!("Loaded DJ config overrides");

    let (shutdown_tx, _shutdown_rx) = tokio::sync::broadcast::channel(16);

    // Initialize metrics if configured
    let metrics_handle = metrics::create_metrics_handle();
    let _metrics_provider = if let (Ok(metrics_url), Ok(api_key)) = (
        std::env::var("GRAFANA_METRICS_URL"),
        std::env::var("GRAFANA_API_KEY"),
    ) {
        match metrics::init_metrics(metrics_url, api_key) {
            Ok((provider, bot_metrics)) => {
                tracing::info!("Metrics initialized successfully");
                *metrics_handle.write().await = Some(bot_metrics);

                // Start heartbeat task
                metrics::start_heartbeat_task(metrics_handle.clone()).await;
                Some(provider)
            }
            Err(e) => {
                tracing::warn!(
                    "Failed to initialize metrics: {}. Continuing without metrics.",
                    e
                );
                None
            }
        }
    } else {
        tracing::info!("Metrics not configured (GRAFANA_METRICS_URL and GRAFANA_API_KEY not set)");
        None
    };

    let data = Arc::new(BotState::new(
        content_path,
        dj_config_overrides_store,
        state_store,
        shutdown_tx,
        metrics_handle.clone(),
    ));

    // Start gauge update task if metrics are configured
    if metrics_handle.read().await.is_some() {
        metrics::start_gauge_update_task(metrics_handle.clone(), data.clone()).await;
    }

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
                commands::messaging::register_channel(),
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

    let intents =
        GatewayIntents::GUILDS | GatewayIntents::GUILD_VOICE_STATES | GatewayIntents::GUILD_MEMBERS;

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

    // Start web server in background
    let web_data = data.clone();
    tokio::spawn(async move {
        if let Err(e) = web::run_web_server(web_data, web_port).await {
            tracing::error!("Web server error: {}", e);
        }
    });

    client.start().await?;

    Ok(())
}
