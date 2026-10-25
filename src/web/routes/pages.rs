use axum::extract::Path;
use axum::http::StatusCode;
use axum::http::header::CONTENT_TYPE;
use axum::response::IntoResponse;
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "static/"]
#[exclude = "*.md"]
struct StaticFiles;

fn serve(path: &str) -> Result<impl IntoResponse + use<>, StatusCode> {
    let file = StaticFiles::get(path).ok_or(StatusCode::NOT_FOUND)?;
    let content_type = format!("{}; charset=utf-8", file.metadata.mimetype());
    Ok(([(CONTENT_TYPE, content_type)], file.data))
}

pub async fn index() -> Result<impl IntoResponse, StatusCode> {
    serve("index.html")
}

pub async fn logs() -> Result<impl IntoResponse, StatusCode> {
    serve("logs.html")
}

pub async fn static_file(Path(path): Path<String>) -> Result<impl IntoResponse, StatusCode> {
    if path.ends_with(".html") {
        return Err(StatusCode::NOT_FOUND);
    }
    serve(&path)
}
