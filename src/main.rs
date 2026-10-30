mod activity;
mod audio;
mod bot_profile;
mod bucket;
mod commands;
mod handlers;
mod logging;
mod metrics;
mod persistence;
mod shutdown;
mod startup;
mod state;
mod web;

use crate::audio::dj::config::DJConfigRegistry;
use crate::bucket::registry::JsonRegistry;
use crate::state::{BotState, Data};
use anyhow::Context;
use poise::serenity_prelude::{self as serenity, GatewayIntents};
use songbird::SerenityInit;
use std::path::PathBuf;
use std::sync::{Arc, LazyLock};
use std::time::Duration;

/// Discord client with the default ratelimiter.
static HTTP: LazyLock<serenity::Http> =
    LazyLock::new(|| serenity::HttpBuilder::new(discord_token()).build());

pub fn http() -> &'static serenity::Http {
    &HTTP
}

pub fn discord_token() -> String {
    std::env::var("DISCORD_TOKEN").expect("Expected DISCORD_TOKEN in the environment")
}

async fn on_error(error: poise::FrameworkError<'_, Data, anyhow::Error>) {
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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
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
                    .add_directive("radio_bot::audio::dj=debug".parse().unwrap())
                    .add_directive("radio_bot::audio::tracks=warn".parse().unwrap())
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
        println!("radio-bot-{} ({})", run_number, commit_hash);
        return Ok(());
    } else {
        tracing::info!("Starting radio-bot-{} ({})", run_number, commit_hash);
    }

    let discord_token = discord_token();
    http();
    web::http();

    let web_port = std::env::var("WEB_PORT")
        .ok()
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(3000);

    let content_path = args
        .get(1)
        .expect("Usage: radio-bot <content_path>")
        .clone();

    tracing::info!("Using content path: {}", content_path);

    let state_store_path = PathBuf::from(
        std::env::var("STATE_STORE_PATH").unwrap_or_else(|_| "./bot-state".to_string()),
    );
    tracing::info!("Using state store path: {:?}", state_store_path);

    let dj_settings_store =
        persistence::DjSettingsStore::new(state_store_path.join("dj_settings.json"));
    tracing::info!("Loaded DJ settings");
    let state_store = Arc::new(persistence::FileStore::new(state_store_path));

    // Initialize metrics if configured
    let _metrics_provider = if let (Ok(metrics_url), Ok(api_key)) = (
        std::env::var("GRAFANA_METRICS_URL"),
        std::env::var("GRAFANA_API_KEY"),
    ) {
        match metrics::init_metrics(metrics_url, api_key) {
            Ok((provider, bot_metrics)) => {
                tracing::info!("Metrics initialized successfully");

                // Start heartbeat task
                metrics::start_heartbeat_task(bot_metrics);
                Some(provider)
            }
            Err(e) => {
                tracing::warn!("Failed to initialize metrics: {e:#}. Continuing without metrics.",);
                None
            }
        }
    } else {
        tracing::info!("Metrics not configured (GRAFANA_METRICS_URL and GRAFANA_API_KEY not set)");
        None
    };

    let bucket = bucket::init_from_env().map(Arc::from);
    if bucket.is_some() {
        tracing::info!("Bucket client initialized");
    } else {
        tracing::info!("Object storage not configured (BUCKET_* env vars not set)");
    }

    let file_cache_dir =
        PathBuf::from(std::env::var("FILE_CACHE_DIR").unwrap_or_else(|_| "./cache".to_string()));
    tracing::info!("Using file cache dir: {:?}", file_cache_dir);

    // Startup is the only thing that deletes cached files, so this defaults
    // to evicting rather than to a cache dir that only ever grows. 0 turns
    // eviction off.
    let file_cache_ttl_secs = std::env::var("FILE_CACHE_TTL_SECS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(30 * 24 * 60 * 60);
    let file_cache_ttl =
        (file_cache_ttl_secs > 0).then(|| Duration::from_secs(file_cache_ttl_secs));
    tracing::info!("Using file cache ttl: {:?}", file_cache_ttl);

    let bucket = match bucket {
        Some(bucket) => Some(
            bucket::Bucket::new(file_cache_dir.clone(), bucket, file_cache_ttl)
                .await
                .with_context(|| format!("Failed to create file cache at {file_cache_dir:?}"))?,
        ),
        None => None,
    };
    let file_resolver = bucket::FileResolver::new(content_path, bucket);

    let bot_user_id = anyhow::Context::context(
        http().get_current_user().await,
        "Failed to fetch the bot user from Discord",
    )?
    .id;

    let signal_profiles_path =
        std::env::var("SIGNAL_PROFILES_PATH").unwrap_or_else(|_| "audio_profiles/".to_string());
    tracing::info!("Using signal profiles path: {}", signal_profiles_path);
    let profile_manager =
        JsonRegistry::from_storage(signal_profiles_path, file_resolver.clone()).await;

    let dj_configs_path =
        std::env::var("DJ_CONFIGS_PATH").unwrap_or_else(|_| "dj_configs/".to_string());
    tracing::info!("Using DJ configs path: {}", dj_configs_path);
    let pools_path = std::env::var("POOLS_PATH").unwrap_or_else(|_| "pools/".to_string());
    tracing::info!("Using pools path: {}", pools_path);
    let dj_config_manager =
        DJConfigRegistry::from_storage(dj_configs_path, pools_path, file_resolver.clone()).await;

    let songbird = songbird::Songbird::serenity();
    let data: Data = Box::leak(Box::new(BotState::new(
        file_resolver,
        state_store,
        dj_settings_store,
        profile_manager,
        dj_config_manager,
        bot_user_id,
        songbird.clone(),
    )));

    // Start gauge update task if metrics are configured
    if let Some(bot_metrics) = metrics::get() {
        metrics::start_gauge_update_task(bot_metrics, data);
    }

    let framework = poise::Framework::builder()
        .options(poise::FrameworkOptions {
            commands: vec![
                commands::voice::join_voice_channel(),
                commands::voice::leave_voice_channel(),
                commands::voice::change_track_state(),
                commands::voice::get_current_tracks(),
                commands::voice::dj(),
                commands::messaging::register_channel(),
                commands::messaging::speak(),
                commands::messaging::set_status(),
            ],
            on_error: |error| Box::pin(on_error(error)),
            ..Default::default()
        })
        .setup(move |ctx, _ready, framework| {
            Box::pin(async move {
                poise::builtins::register_globally(ctx, &framework.options().commands).await?;
                let command_names: Vec<_> = framework
                    .options()
                    .commands
                    .iter()
                    .map(|cmd| cmd.name.as_ref())
                    .collect();
                tracing::info!(
                    "Registered {} commands globally: {}",
                    framework.options().commands.len(),
                    command_names.join(", ")
                );
                Ok(data)
            })
        })
        .build();

    let intents =
        GatewayIntents::GUILDS | GatewayIntents::GUILD_VOICE_STATES | GatewayIntents::GUILD_MEMBERS;

    let mut client = serenity::Client::builder(&discord_token, intents)
        .framework(framework)
        .event_handler(handlers::discord::Handler { data })
        .register_songbird_with(songbird)
        .await?;

    // Set up graceful shutdown handling
    shutdown::setup_shutdown_handler(data).await;

    // Start web server in background
    tokio::spawn(async move {
        if let Err(e) = web::run_web_server(data, web_port).await {
            tracing::error!("Web server error: {e:#}");
        }
    });

    client.start().await?;

    Ok(())
}
