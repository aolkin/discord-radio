use serenity::all::Http;
use std::sync::LazyLock;

pub mod audio_stream;
pub mod routes;
pub mod server;
pub mod state_snapshot;
pub mod websocket;

pub use server::run_web_server;

/// Discord client for web routes; its ratelimiter is off so a ratelimited
/// request fails with 429 instead of waiting.
static HTTP: LazyLock<Http> = LazyLock::new(|| {
    serenity::all::HttpBuilder::new(crate::discord_token())
        .ratelimiter_disabled(true)
        .build()
});

pub fn http() -> &'static Http {
    &HTTP
}
