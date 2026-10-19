use crate::state::Data;
use crate::web::state_snapshot::BotSnapshot;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::http::header::CONTENT_TYPE;
use axum::response::{Html, IntoResponse, Json};
use rust_embed::RustEmbed;

pub async fn index() -> Html<&'static str> {
    Html(include_str!("../../../static/index.html"))
}

pub async fn logs() -> Html<&'static str> {
    Html(include_str!("../../../static/logs.html"))
}

#[derive(RustEmbed)]
#[folder = "static/"]
#[exclude = "*.html"]
#[exclude = "*.md"]
struct StaticFiles;

pub async fn static_file(Path(path): Path<String>) -> Result<impl IntoResponse, StatusCode> {
    let file = StaticFiles::get(&path).ok_or(StatusCode::NOT_FOUND)?;
    let content_type = format!("{}; charset=utf-8", file.metadata.mimetype());
    Ok(([(CONTENT_TYPE, content_type)], file.data))
}

pub async fn get_status(State(bot_state): State<Data>) -> Json<BotSnapshot> {
    let snapshot = BotSnapshot::capture(&bot_state).await;
    Json(snapshot)
}

pub async fn health() -> impl IntoResponse {
    (StatusCode::OK, "OK")
}
