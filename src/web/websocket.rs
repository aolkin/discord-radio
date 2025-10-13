use crate::state::Data;
use crate::web::state_snapshot::BotSnapshot;
use axum::extract::ws::{Message, WebSocket};
use axum::extract::{State, WebSocketUpgrade};
use axum::response::Response;
use tokio::time::{Duration, interval};

pub async fn websocket_handler(ws: WebSocketUpgrade, State(bot_state): State<Data>) -> Response {
    ws.on_upgrade(|socket| handle_socket(socket, bot_state))
}

async fn handle_socket(mut socket: WebSocket, bot_state: Data) {
    let mut interval = interval(Duration::from_secs(1));

    loop {
        interval.tick().await;

        let snapshot = BotSnapshot::capture(&bot_state).await;
        let json = match serde_json::to_string(&snapshot) {
            Ok(j) => j,
            Err(e) => {
                tracing::error!("Failed to serialize snapshot: {}", e);
                continue;
            }
        };

        if socket.send(Message::Text(json.into())).await.is_err() {
            tracing::debug!("WebSocket client disconnected");
            break;
        }
    }
}
