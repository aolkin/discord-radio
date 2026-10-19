use crate::state::Data;
use crate::web::{audio_stream, routes, websocket};
use axum::{
    Router,
    extract::{DefaultBodyLimit, RawPathParams, Request},
    middleware::{self, Next},
    response::Response,
    routing::{get, post},
};
use http::header::{AUTHORIZATION, CONTENT_TYPE};
use std::net::SocketAddr;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tracing::Instrument;

async fn guild_span(params: RawPathParams, request: Request, next: Next) -> Response {
    let guild_id = params
        .iter()
        .find(|(name, _)| *name == "guild_id")
        .map(|(_, value)| value.to_string());
    match guild_id {
        Some(guild_id) => {
            next.run(request)
                .instrument(tracing::info_span!("request", guild_id))
                .await
        }
        None => next.run(request).await,
    }
}

pub async fn run_web_server(bot_state: Data, port: u16) -> anyhow::Result<()> {
    // Configure CORS - secure by default (localhost only), configurable via env var
    let localhost_predicate = AllowOrigin::predicate(|origin: &http::header::HeaderValue, _| {
        origin.as_bytes().starts_with(b"http://localhost:")
            || origin.as_bytes().starts_with(b"http://127.0.0.1:")
    });

    let cors = if let Ok(allowed_origins) = std::env::var("WEB_CORS_ALLOWED_ORIGINS") {
        // Parse comma-separated origins
        let origins: Vec<_> = allowed_origins
            .split(',')
            .filter_map(|s| s.trim().parse().ok())
            .collect();

        if origins.is_empty() {
            tracing::warn!(
                "WEB_CORS_ALLOWED_ORIGINS set but no valid origins found, using localhost only"
            );
            CorsLayer::new().allow_origin(localhost_predicate)
        } else {
            CorsLayer::new().allow_origin(AllowOrigin::list(origins))
        }
    } else {
        // Default: only allow localhost
        CorsLayer::new().allow_origin(localhost_predicate)
    }
    .allow_methods(tower_http::cors::Any)
    .allow_headers([CONTENT_TYPE, AUTHORIZATION]);

    let app = Router::new()
        .route("/", get(routes::pages::index))
        .route("/logs/", get(routes::pages::logs))
        .route("/static/{*path}", get(routes::pages::static_file))
        .route("/api/status", get(routes::status::get_status))
        .route("/api/health", get(routes::status::health))
        .route(
            "/api/guilds/{guild_id}/dj/advance",
            post(routes::dj::advance_dj_state),
        )
        .route(
            "/api/guilds/{guild_id}/hex/play",
            post(routes::dj::play_hex_message),
        )
        .route(
            "/api/guilds/{guild_id}/dj/idle",
            post(routes::dj::force_dj_idle),
        )
        .route(
            "/api/guilds/{guild_id}/track/play",
            post(routes::dj::play_track),
        )
        .route(
            "/api/guilds/{guild_id}/tracks",
            post(routes::tracks::change_track_state),
        )
        .route(
            "/api/guilds/{guild_id}/dj/profile",
            post(routes::dj::set_dj_background_profile),
        )
        .route(
            "/api/guilds/{guild_id}/dj/profile-override",
            post(routes::dj::set_dj_override_profile),
        )
        .route(
            "/api/guilds/{guild_id}/dj/channel-status",
            post(routes::dj::set_dj_channel_status),
        )
        .route(
            "/api/guilds/{guild_id}/bot-profile",
            get(routes::discord::get_bot_profile).put(routes::discord::set_bot_profile),
        )
        .route("/api/profiles", get(routes::profiles::get_profiles))
        .route(
            "/api/profiles/{name}",
            get(routes::profiles::get_profile).put(routes::profiles::put_profile),
        )
        .route("/api/dj-configs", get(routes::dj_configs::get_dj_configs))
        .route(
            "/api/dj-configs/{name}",
            get(routes::dj_configs::get_dj_config).put(routes::dj_configs::put_dj_config),
        )
        .route("/api/files", get(routes::files::browse_files))
        .route(
            "/api/files/upload",
            post(routes::files::upload_file)
                .layer(DefaultBodyLimit::max(routes::files::MAX_UPLOAD_BYTES)),
        )
        .route("/api/files/folders", post(routes::files::create_folder))
        .route(
            "/api/activity/set",
            post(routes::activity::set_bot_activity),
        )
        .route(
            "/api/guilds/{guild_id}/audio-stream",
            get(audio_stream::audio_stream),
        )
        .route(
            "/api/guilds/{guild_id}/logs/{log_type}",
            get(routes::logs::get_logs),
        )
        .route(
            "/api/registered-channels",
            get(routes::discord::get_registered_channels),
        )
        .route(
            "/api/registered-channels/{channel_id}/send",
            post(routes::discord::send_channel_message),
        )
        .route("/ws", get(websocket::websocket_handler))
        .route_layer(middleware::from_fn(guild_span))
        .layer(cors)
        .with_state(bot_state);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!("Web portal listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
