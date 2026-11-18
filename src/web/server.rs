use crate::audio::dj::config::{DJConfig, Pool};
use crate::audio::profiles::SignalProfile;
use crate::state::Data;
use crate::web::routes::registries;
use crate::web::{audio_stream, routes, websocket};
use anyhow::Context;
use axum::{
    Router,
    extract::{DefaultBodyLimit, Path, Request, State},
    middleware::{self, Next},
    response::Response,
    routing::{delete, get, post},
};
use http::header::{AUTHORIZATION, CONTENT_TYPE};
use serenity::model::id::GuildId;
use std::net::SocketAddr;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tracing::Instrument;

#[derive(serde::Deserialize)]
struct GuildPath {
    guild_id: u64,
}

async fn guild_scope(
    State(bot_state): State<Data>,
    Path(GuildPath { guild_id }): Path<GuildPath>,
    request: Request,
    next: Next,
) -> Response {
    tracing::Span::current().record("guild_id", guild_id);
    if !request.method().is_safe() {
        bot_state.touch(GuildId::new(guild_id));
    }
    next.run(request).await
}

async fn request_span(request: Request, next: Next) -> Response {
    let span = tracing::info_span!(
        "request",
        method = %request.method(),
        path = request.uri().path(),
        guild_id = tracing::field::Empty,
    );
    next.run(request).instrument(span).await
}

pub async fn run_web_server(bot_state: Data, port: u16, bot_name: &str) -> anyhow::Result<()> {
    let index =
        routes::pages::render_index(bot_name).context("Failed to render the portal page")?;

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

    let guild_routes = Router::new()
        .route("/load", post(routes::discord::load_guild))
        .route("/unload", post(routes::discord::unload_guild))
        .route("/dj/advance", post(routes::dj::advance_dj_state))
        .route("/hex/play", post(routes::dj::play_hex_message))
        .route("/dj/idle", post(routes::dj::force_dj_idle))
        .route("/track/play", post(routes::dj::play_track))
        .route("/tracks", post(routes::tracks::change_track_state))
        .route("/dj/config", post(routes::dj::configure_dj))
        .route("/dj/profile", post(routes::dj::set_dj_profile))
        .route(
            "/dj/channel-status",
            post(routes::dj::set_dj_channel_status),
        )
        .route(
            "/dj/announcement-channels",
            post(routes::dj::add_announcement_channel),
        )
        .route(
            "/dj/announcement-channels/{channel_id}",
            delete(routes::dj::remove_announcement_channel),
        )
        .route(
            "/bot-profile",
            get(routes::discord::get_bot_profile).put(routes::discord::set_bot_profile),
        )
        .route("/audio-stream", get(audio_stream::audio_stream))
        .route("/logs/{log_type}", get(routes::logs::get_logs))
        .route("/channels", get(routes::discord::get_sendable_channels))
        .route(
            "/channels/{channel_id}/send",
            post(routes::discord::send_channel_message),
        )
        .route("/voice-channels", get(routes::discord::get_voice_channels))
        .route(
            "/voice-channels/leave",
            post(routes::discord::leave_voice_channel),
        )
        .route(
            "/voice-channels/{channel_id}/join",
            post(routes::discord::join_voice_channel),
        )
        .route_layer(middleware::from_fn_with_state(bot_state, guild_scope));

    let app = Router::new()
        .route("/", get(move || std::future::ready(index.clone())))
        .route("/static/{*path}", get(routes::pages::static_file))
        .route("/api/status", get(routes::status::get_status))
        .route("/api/health", get(routes::status::health))
        .route("/api/guilds", get(routes::discord::get_guilds))
        .nest("/api/guilds/{guild_id}", guild_routes)
        .merge(registries::routes::<SignalProfile>())
        .merge(registries::routes::<DJConfig>())
        .merge(registries::routes::<Pool>())
        .route(
            "/api/files",
            get(routes::files::browse_files).delete(routes::files::delete_file),
        )
        .route(
            "/api/hex-audio-folder",
            get(routes::hex_folder::check_hex_folder),
        )
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
        .route("/ws", get(websocket::websocket_handler))
        .route_layer(middleware::from_fn(request_span))
        .layer(cors)
        .with_state(bot_state);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!("Web portal listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use http::StatusCode;
    use tower::ServiceExt;

    async fn scope(
        Path(GuildPath { guild_id }): Path<GuildPath>,
        request: Request,
        next: Next,
    ) -> Response {
        assert_eq!(guild_id, 42);
        next.run(request).await
    }

    async fn status_of(uri: &str) -> StatusCode {
        let guild_routes = Router::new()
            .route("/one", get(|| async {}))
            .route("/two/{channel_id}", get(|| async {}))
            .route_layer(middleware::from_fn(scope));
        Router::new()
            .nest("/api/guilds/{guild_id}", guild_routes)
            .oneshot(Request::get(uri).body(Body::empty()).unwrap())
            .await
            .unwrap()
            .status()
    }

    #[tokio::test]
    async fn guild_path_extracts_with_extra_path_parameters() {
        assert_eq!(status_of("/api/guilds/42/one").await, StatusCode::OK);
        assert_eq!(status_of("/api/guilds/42/two/7").await, StatusCode::OK);
        assert_eq!(
            status_of("/api/guilds/abc/two/7").await,
            StatusCode::BAD_REQUEST
        );
    }
}
