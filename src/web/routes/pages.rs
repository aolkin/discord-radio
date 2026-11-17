use axum::extract::Path;
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE, ETAG, IF_NONE_MATCH};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use rust_embed::RustEmbed;
use sha2::{Digest, Sha256};
use std::fmt::Write;
use std::sync::Arc;

#[derive(RustEmbed)]
#[folder = "static/"]
#[exclude = "*.md"]
struct StaticFiles;

const BOT_NAME_PLACEHOLDER: &str = "{{BOT_NAME}}";

#[derive(Clone)]
pub struct IndexPage {
    html: Arc<str>,
    etag: Arc<str>,
}

fn etag_from_hash(hash: &[u8]) -> String {
    let mut etag = String::with_capacity(hash.len() * 2 + 2);
    etag.push('"');
    for byte in hash {
        write!(etag, "{byte:02x}").expect("writing to a String cannot fail");
    }
    etag.push('"');
    etag
}

fn matches_if_none_match(headers: &HeaderMap, etag: &str) -> bool {
    headers
        .get_all(IF_NONE_MATCH)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .map(str::trim)
        .any(|candidate| candidate == "*" || candidate.trim_start_matches("W/") == etag)
}

fn respond(headers: &HeaderMap, etag: &str, content_type: String, body: Vec<u8>) -> Response {
    let etag_value = HeaderValue::from_str(etag).expect("etag contains only ASCII characters");
    let validators = [
        (CACHE_CONTROL, HeaderValue::from_static("no-cache")),
        (ETAG, etag_value),
    ];
    if matches_if_none_match(headers, etag) {
        return (StatusCode::NOT_MODIFIED, validators).into_response();
    }
    (validators, [(CONTENT_TYPE, content_type)], body).into_response()
}

fn serve(headers: &HeaderMap, path: &str) -> Result<Response, StatusCode> {
    let file = StaticFiles::get(path).ok_or(StatusCode::NOT_FOUND)?;
    let content_type = format!("{}; charset=utf-8", file.metadata.mimetype());
    let etag = etag_from_hash(&file.metadata.sha256_hash());
    Ok(respond(
        headers,
        &etag,
        content_type,
        file.data.into_owned(),
    ))
}

fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

pub fn render_index(bot_name: &str) -> anyhow::Result<IndexPage> {
    let file = StaticFiles::get("index.html")
        .ok_or_else(|| anyhow::anyhow!("index.html is missing from the embedded files"))?;
    let html = std::str::from_utf8(&file.data)?;
    let html = html.replace(BOT_NAME_PLACEHOLDER, &escape_html(bot_name));
    let etag = etag_from_hash(&Sha256::digest(html.as_bytes()));
    Ok(IndexPage {
        html: html.into(),
        etag: etag.into(),
    })
}

pub async fn index(page: IndexPage, headers: HeaderMap) -> Response {
    respond(
        &headers,
        &page.etag,
        "text/html; charset=utf-8".to_string(),
        page.html.as_bytes().to_vec(),
    )
}

pub async fn static_file(
    Path(path): Path<String>,
    headers: HeaderMap,
) -> Result<Response, StatusCode> {
    if path.ends_with(".html") {
        return Err(StatusCode::NOT_FOUND);
    }
    serve(&headers, &path)
}
