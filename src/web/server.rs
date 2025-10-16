use crate::state::Data;
use crate::web::{audio_stream, routes, websocket};
use axum::{
    Router,
    routing::{get, post},
};
use http::header::{AUTHORIZATION, CONTENT_TYPE};
use std::net::SocketAddr;
use tower_http::cors::{AllowOrigin, CorsLayer};

pub async fn run_web_server(
    bot_state: Data,
    port: u16,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
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
        .route("/", get(routes::index))
        .route("/logs.html", get(routes::logs))
        .route("/api/status", get(routes::get_status))
        .route("/api/health", get(routes::health))
        .route(
            "/api/guilds/{guild_id}/dj/advance",
            post(routes::advance_dj_state),
        )
        .route(
            "/api/guilds/{guild_id}/hex/play",
            post(routes::play_hex_message),
        )
        .route(
            "/api/guilds/{guild_id}/tracks",
            post(routes::change_track_state),
        )
        .route(
            "/api/guilds/{guild_id}/profile",
            post(routes::signal_profile),
        )
        .route("/api/profiles", get(routes::get_profiles))
        .route("/api/audio-files", get(routes::get_audio_files))
        .route("/api/activity/set", post(routes::set_bot_activity))
        .route("/api/activity/push", post(routes::push_bot_activity))
        .route("/api/activity/remove", post(routes::remove_bot_activity))
        .route(
            "/api/guilds/{guild_id}/audio-stream",
            get(audio_stream::audio_stream),
        )
        .route(
            "/api/dj-config/overrides",
            get(routes::get_dj_config_overrides),
        )
        .route(
            "/api/dj-config/overrides/hex-messages",
            post(routes::set_hex_message_override),
        )
        .route(
            "/api/dj-config/overrides/hex-messages/{index}",
            axum::routing::delete(routes::delete_hex_message_override),
        )
        .route(
            "/api/dj-config/overrides/announcements",
            post(routes::set_announcement_override),
        )
        .route(
            "/api/dj-config/overrides/announcements/{index}",
            axum::routing::delete(routes::delete_announcement_override),
        )
        .route(
            "/api/dj-config/overrides/toggle",
            post(routes::toggle_override_category),
        )
        .route(
            "/api/dj-config/overrides/state-weights",
            post(routes::set_state_weights_override),
        )
        .route(
            "/api/dj-config/default-state-weights",
            get(routes::get_default_state_weights),
        )
        .route(
            "/api/guilds/{guild_id}/logs/{log_type}",
            get(routes::get_logs),
        )
        .route("/ws", get(websocket::websocket_handler))
        .layer(cors)
        .with_state(bot_state);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!("Web portal listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
