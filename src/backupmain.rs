use axum::{
    extract::ws::{
        Message, //Represents a WebSocket frame. supports text, binary, ping, pong, close.
        WebSocket, //The actual full-duplex socket. After upgrade, this is what you use. supports send, receive ,split.
        WebSocketUpgrade //without this, cannot perform WebSocket handshake. 
        //Represents an incoming HTTP request that wants to upgrade to WebSocket.
        //Converts HTTP → WebSocket protocol.
    },
    response::IntoResponse,//trait| Anything that implements IntoResponse can be returned from an Axum handler.
    // ws.on_upgrade(...) returns a type that implements IntoResponse.
    routing::get,//Registers HTTP GET route. WebSocket handshake always starts as HTTP GET request.
    Router,//A router is a collection of routes.Without Router: 👉 No route definitions.
};
use futures_util::{
    SinkExt,//SplitSink implements Sink, but .send() is provided by SinkExt.
    // Without it:
    // ❌ .send() method doesn't exist.
     StreamExt// WebSocket implements Stream, but .next() is provided by StreamExt.
     // Without StreamExt:
     // ❌ .next() will not compile.
    };


use std::net::SocketAddr;//SocketAddr is a tuple of (ip_address, port).
use std::sync::Arc;//Atomic Reference Counted pointer. Without Arc:
// ❌ Cannot move sender into multiple async contexts.
use tokio::sync::Mutex;
// IMPORTANT:
// This is async mutex, not std::sync::Mutex.
// Why? Because:
// We are inside async functions.
// std::Mutex blocks thread.
// tokio::Mutex yields control when waiting.

// Type alias for client sender| A sender is a half of a split WebSocket.
type Client = Arc<Mutex<futures_util::stream::SplitSink<WebSocket, Message>>>;

#[tokio::main]
async fn main() {
    let app = Router::new().route("/ws", get(ws_handler));

    let addr = SocketAddr::from(([127, 0, 0, 1], 7878));
    println!("WebSocket server running on ws://{addr}/ws");

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

// WebSocket route handler trait| Anything that implements IntoResponse can be returned from an Axum handler.
// ws.on_upgrade(...) returns a type that implements IntoResponse.
async fn ws_handler(ws: WebSocketUpgrade) -> impl IntoResponse {
    println!("WebSocket upgrade requested");
    ws.on_upgrade(handle_socket)
}

// Actual WebSocket logic
async fn handle_socket(socket: WebSocket) {
    println!("Client connected");

    let (sender, mut receiver) = socket.split();

    let client: Client = Arc::new(Mutex::new(sender));

    // Send welcome message
    let welcome_msg = serde_json::json!({
        "server_method": "system",
        "data": {
            "message": "Connected to WebSocket server."
        }
    });

    {
        let mut locked = client.lock().await;
        let _ = locked
            .send(Message::Text(welcome_msg.to_string()))
            .await;
    }

    // Receive loop
    while let Some(msg_result) = receiver.next().await {
        let msg = match msg_result {
            Ok(msg) => msg,
            Err(_) => break,
        };

        match msg {
            Message::Text(text) => {
                println!("Received: {}", text);

                let response = format!("Echo: {}", text);

                let mut locked = client.lock().await;
                let _ = locked.send(Message::Text(response)).await;
            }

            Message::Binary(data) => {
                let mut locked = client.lock().await;
                let _ = locked.send(Message::Binary(data)).await;
            }

            Message::Ping(payload) => {
                let mut locked = client.lock().await;
                let _ = locked.send(Message::Pong(payload)).await;
            }

            Message::Pong(_) => {}

            Message::Close(frame) => {
                let mut locked = client.lock().await;
                let _ = locked.send(Message::Close(frame)).await;
                break;
            }
        }
    }

    println!("Client disconnected");
}
