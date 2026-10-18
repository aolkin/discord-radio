use crate::state::Data;
use crate::web::state_snapshot::BotSnapshot;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::http::header::CONTENT_TYPE;
use axum::response::{Html, IntoResponse, Json};

pub async fn index() -> Html<&'static str> {
    Html(include_str!("../../../static/index.html"))
}

pub async fn logs() -> Html<&'static str> {
    Html(include_str!("../../../static/logs.html"))
}

pub async fn static_file(Path(path): Path<String>) -> Result<impl IntoResponse, StatusCode> {
    let (content_type, body) = match path.as_str() {
        "base.css" => (
            "text/css; charset=utf-8",
            include_str!("../../../static/base.css"),
        ),
        "logs.css" => (
            "text/css; charset=utf-8",
            include_str!("../../../static/logs.css"),
        ),
        "index.css" => (
            "text/css; charset=utf-8",
            include_str!("../../../static/index.css"),
        ),
        "alpine.min.js" => (
            "text/javascript; charset=utf-8",
            include_str!("../../../static/alpine.min.js"),
        ),
        "index.js" => (
            "text/javascript; charset=utf-8",
            include_str!("../../../static/index.js"),
        ),
        "js/activity.js" => (
            "text/javascript; charset=utf-8",
            include_str!("../../../static/js/activity.js"),
        ),
        "js/api.js" => (
            "text/javascript; charset=utf-8",
            include_str!("../../../static/js/api.js"),
        ),
        "js/bot-profile.js" => (
            "text/javascript; charset=utf-8",
            include_str!("../../../static/js/bot-profile.js"),
        ),
        "js/channels.js" => (
            "text/javascript; charset=utf-8",
            include_str!("../../../static/js/channels.js"),
        ),
        "js/connection.js" => (
            "text/javascript; charset=utf-8",
            include_str!("../../../static/js/connection.js"),
        ),
        "js/dj.js" => (
            "text/javascript; charset=utf-8",
            include_str!("../../../static/js/dj.js"),
        ),
        "js/file-browser.js" => (
            "text/javascript; charset=utf-8",
            include_str!("../../../static/js/file-browser.js"),
        ),
        "js/modal.js" => (
            "text/javascript; charset=utf-8",
            include_str!("../../../static/js/modal.js"),
        ),
        "js/run-action.js" => (
            "text/javascript; charset=utf-8",
            include_str!("../../../static/js/run-action.js"),
        ),
        "js/signal-profiles.js" => (
            "text/javascript; charset=utf-8",
            include_str!("../../../static/js/signal-profiles.js"),
        ),
        "js/tracks.js" => (
            "text/javascript; charset=utf-8",
            include_str!("../../../static/js/tracks.js"),
        ),
        _ => return Err(StatusCode::NOT_FOUND),
    };
    Ok(([(CONTENT_TYPE, content_type)], body))
}

pub async fn get_status(State(bot_state): State<Data>) -> Json<BotSnapshot> {
    let snapshot = BotSnapshot::capture(&bot_state).await;
    Json(snapshot)
}

pub async fn health() -> impl IntoResponse {
    (StatusCode::OK, "OK")
}
