use crate::state::Data;
use crate::web::{routes, websocket};
use axum::{Router, routing::get};
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
    .allow_methods([http::Method::GET])
    .allow_headers([CONTENT_TYPE, AUTHORIZATION]);

    let app = Router::new()
        .route("/", get(routes::index))
        .route("/api/status", get(routes::get_status))
        .route("/api/health", get(routes::health))
        .route("/ws", get(websocket::websocket_handler))
        .layer(cors)
        .with_state(bot_state);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!("Web portal listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
