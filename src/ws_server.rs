use axum::{
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    response::IntoResponse,
};

// Public handler that http_server.rs can use for the /ws route
pub async fn ws_handler(ws: WebSocketUpgrade) -> impl IntoResponse {
    ws.on_upgrade(handle_socket)
}

// Internal function that actually processes WebSocket messages
async fn handle_socket(mut socket: WebSocket) {
    while let Some(Ok(msg)) = socket.recv().await {
        let reply = match msg {
            Message::Text(text) => Message::Text(format!("echo: {text}")),
            Message::Binary(data) => Message::Binary(data),
            Message::Ping(payload) => Message::Pong(payload),
            Message::Pong(_) => continue,
            Message::Close(frame) => {
                let _ = socket.send(Message::Close(frame)).await;
                break;
            }
        };

        if socket.send(reply).await.is_err() {
            break;
        }
    }
}

