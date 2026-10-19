use crate::state::Data;
use crate::web::state_snapshot::BotSnapshot;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json};

pub async fn get_status(State(bot_state): State<Data>) -> Json<BotSnapshot> {
    let snapshot = BotSnapshot::capture(&bot_state).await;
    Json(snapshot)
}

pub async fn health() -> impl IntoResponse {
    (StatusCode::OK, "OK")
}
