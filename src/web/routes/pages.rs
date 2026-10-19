use axum::extract::Path;
use axum::http::StatusCode;
use axum::http::header::CONTENT_TYPE;
use axum::response::{Html, IntoResponse};
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
