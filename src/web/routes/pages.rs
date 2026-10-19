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

const STATIC_FILES: &[(&str, &str)] = &[
    ("base.css", include_str!("../../../static/base.css")),
    ("logs.css", include_str!("../../../static/logs.css")),
    ("index.css", include_str!("../../../static/index.css")),
    (
        "alpine.min.js",
        include_str!("../../../static/alpine.min.js"),
    ),
    ("index.js", include_str!("../../../static/index.js")),
    (
        "js/activity.js",
        include_str!("../../../static/js/activity.js"),
    ),
    ("js/api.js", include_str!("../../../static/js/api.js")),
    (
        "js/bot-profile.js",
        include_str!("../../../static/js/bot-profile.js"),
    ),
    (
        "js/channels.js",
        include_str!("../../../static/js/channels.js"),
    ),
    (
        "js/connection.js",
        include_str!("../../../static/js/connection.js"),
    ),
    ("js/dj.js", include_str!("../../../static/js/dj.js")),
    (
        "js/file-browser.js",
        include_str!("../../../static/js/file-browser.js"),
    ),
    ("js/modal.js", include_str!("../../../static/js/modal.js")),
    (
        "js/run-action.js",
        include_str!("../../../static/js/run-action.js"),
    ),
    (
        "js/signal-profiles.js",
        include_str!("../../../static/js/signal-profiles.js"),
    ),
    ("js/tracks.js", include_str!("../../../static/js/tracks.js")),
];

pub async fn static_file(Path(path): Path<String>) -> Result<impl IntoResponse, StatusCode> {
    let (_, body) = STATIC_FILES
        .iter()
        .find(|(name, _)| *name == path)
        .ok_or(StatusCode::NOT_FOUND)?;
    let content_type = if path.ends_with(".css") {
        "text/css; charset=utf-8"
    } else {
        "text/javascript; charset=utf-8"
    };
    Ok(([(CONTENT_TYPE, content_type)], *body))
}

pub async fn get_status(State(bot_state): State<Data>) -> Json<BotSnapshot> {
    let snapshot = BotSnapshot::capture(&bot_state).await;
    Json(snapshot)
}

pub async fn health() -> impl IntoResponse {
    (StatusCode::OK, "OK")
}
