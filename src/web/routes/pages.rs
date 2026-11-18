use axum::extract::Path;
use axum::http::StatusCode;
use axum::http::header::CONTENT_TYPE;
use axum::response::{Html, IntoResponse};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "portal/static/"]
struct StaticFiles;

#[derive(RustEmbed)]
#[folder = "$OUT_DIR/bundle"]
struct BundleFiles;

const BOT_NAME_PLACEHOLDER: &str = "{{BOT_NAME}}";

fn serve(path: &str) -> Result<impl IntoResponse + use<>, StatusCode> {
    let file = BundleFiles::get(path)
        .or_else(|| StaticFiles::get(path))
        .ok_or(StatusCode::NOT_FOUND)?;
    let content_type = format!("{}; charset=utf-8", file.metadata.mimetype());
    Ok(([(CONTENT_TYPE, content_type)], file.data))
}

fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

pub fn render_index(bot_name: &str) -> anyhow::Result<Html<String>> {
    let file = BundleFiles::get("index.html")
        .ok_or_else(|| anyhow::anyhow!("index.html is missing from the embedded files"))?;
    let html = std::str::from_utf8(&file.data)?;
    Ok(Html(
        html.replace(BOT_NAME_PLACEHOLDER, &escape_html(bot_name)),
    ))
}

pub async fn static_file(Path(path): Path<String>) -> Result<impl IntoResponse, StatusCode> {
    if path.ends_with(".html") {
        return Err(StatusCode::NOT_FOUND);
    }
    serve(&path)
}
