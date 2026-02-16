use axum::{
    extract::{
        ws::{
            Message as WsMessage, //Represents a WebSocket frame. supports text, binary, ping, pong, close.
            WebSocket, //The actual full-duplex socket. After upgrade, this is what you use. supports send, receive ,split.
            WebSocketUpgrade, //without this, cannot perform WebSocket handshake. 
            //Represents an incoming HTTP request that wants to upgrade to WebSocket.
            //Converts HTTP → WebSocket protocol.
        },
        Query,
        State,
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
use std::collections::HashMap;
use tokio::sync::Mutex;
// IMPORTANT:
// This is async mutex, not std::sync::Mutex.
// Why? Because:
// We are inside async functions.
// std::Mutex blocks thread.
// tokio::Mutex yields control when waiting.

// Include generated protobuf code
pub mod generated {
    include!("generated/messages.rs");
}
use generated::*;
use prost::Message; // Trait for encode/decode methods

// Type alias for client sender| A sender is a half of a split WebSocket.
type Client = Arc<Mutex<futures_util::stream::SplitSink<WebSocket, WsMessage>>>;

// Peer information structure
#[allow(dead_code)]
struct Peer {
    sender: Client,
    display_name: String,
    peer_id: String, // Kept for future use (e.g., peer lookup, admin features)
}

// Global state to store all connected peers
// Key: peer_id, Value: Peer struct
type Peers = Arc<Mutex<HashMap<String, Peer>>>;

// Helper to send any Envelope with consistent logging
async fn send_server_message(client: &Client, msg: &Envelope, context: &str) {
    println!(
        "[SERVER DEBUG] [{}] Preparing to send Envelope: {:?}",
        context, msg
    );
    let bytes = msg.encode_to_vec();
    println!(
        "[SERVER DEBUG] [{}] Encoded Envelope ({} bytes)",
        context,
        bytes.len()
    );
    let mut sender_lock = client.lock().await;
    match sender_lock.send(WsMessage::Binary(bytes.into())).await {
        Ok(_) => println!("[SERVER DEBUG] [{}] ✅ Send OK", context),
        Err(e) => println!("[SERVER DEBUG] [{}] ❌ Send failed: {}", context, e),
    }
}

#[tokio::main]
async fn main() {
    // Create shared state for all peers
    let peers: Peers = Arc::new(Mutex::new(HashMap::new()));

    let app = Router::new()
        .route("/ws", get(ws_handler))
        .with_state(peers);

    let addr = SocketAddr::from(([127, 0, 0, 1], 7878));
    println!("WebSocket server running on ws://{addr}/ws");

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

// WebSocket route handler
// Extracts query params and shared state, then upgrades to WebSocket
async fn ws_handler(
    Query(params): Query<HashMap<String, String>>,
    ws: WebSocketUpgrade,
    State(peers): State<Peers>,
) -> impl IntoResponse {
    println!("WebSocket upgrade requested");

    // Read displayName and peerId from query parameters
    let display_name = params
        .get("displayName")
        .cloned()
        .unwrap_or_else(|| "Anonymous".to_string());

    let peer_id = params
        .get("peerId")
        .cloned()
        .unwrap_or_else(|| {
            format!(
                "peer_{}",
                uuid::Uuid::new_v4().to_string().split('-').next().unwrap_or("unknown")
            )
        });

    println!(
        "[SERVER] Using client-provided identity: display_name='{}', peer_id='{}'",
        display_name, peer_id
    );

    ws.on_upgrade(move |socket| handle_socket(socket, peers, display_name, peer_id))
}

// Actual WebSocket logic
async fn handle_socket(socket: WebSocket, peers: Peers, display_name: String, peer_id: String) {
    println!("[SERVER] WebSocket upgrade completed - client connected");

    let (sender, mut receiver) = socket.split();
    let client: Client = Arc::new(Mutex::new(sender));

    // Add peer to the shared state
    let peer_count_after_join: usize;
    {
        let mut peers_guard = peers.lock().await;
        peers_guard.insert(
            peer_id.clone(),
            Peer {
                sender: client.clone(),
                display_name: display_name.clone(),
                peer_id: peer_id.clone(),
            },
        );
        peer_count_after_join = peers_guard.len();
        println!("[SERVER] ✅ Peer registered: {} ({})", display_name, peer_id);
        println!("[SERVER] Total connected peers: {}", peer_count_after_join);
    }

    // Broadcast \"peer_joined\" notification to all OTHER peers (not the new peer)
    let mut join_data = std::collections::HashMap::new();
    join_data.insert("peerId".to_string(), peer_id.clone());
    join_data.insert("displayName".to_string(), display_name.clone());
    join_data.insert("message".to_string(), format!("{} joined", display_name));

    let join_notification = Envelope {
        event: "notification".to_string(),
        event_data: Some(EventData {
            method: "peer_joined".to_string(),
            data: join_data,
        }),
    };

    {
        let peers_guard = peers.lock().await;
        for (id, peer) in peers_guard.iter() {
            // Skip the newly joined peer - only notify others
            if *id != peer_id {
                let ctx = format!("join_notification → {}", id);
                send_server_message(&peer.sender, &join_notification, &ctx).await;
            }
        }
    }

    // Receive loop
    while let Some(msg_result) = receiver.next().await {
        let msg = match msg_result {
            Ok(msg) => msg,
            Err(_) => break,
        };

        match msg {
            WsMessage::Binary(data) => {
                println!(
                    "[SERVER DEBUG] 📥 Raw binary frame from client ({} bytes)",
                    data.len()
                );
                // Parse protobuf envelope from client
                match Envelope::decode(data.as_ref()) {
                    Ok(envelope) => {
                        println!("[SERVER DEBUG] Decoded client Envelope: {:?}", envelope);

                        // We only expect \"request\" from client
                        if envelope.event != "request" {
                            println!("[SERVER DEBUG] Unexpected event from client: {}", envelope.event);
                            continue;
                        }

                        let Some(event_data) = envelope.event_data else {
                            println!("[SERVER DEBUG] Missing event_data in client envelope");
                            continue;
                        };

                        let method = event_data.method;
                        let data = event_data.data;

                        if method == "chat_message" {
                            let sender_display_name =
                                data.get("displayName").cloned().unwrap_or_else(|| display_name.clone());
                            let text = data.get("text").cloned().unwrap_or_default();

                            println!(
                                "Received chat_message from {} ({}): {}",
                                sender_display_name, peer_id, text
                            );

                            // Broadcast as notification chat_message to all OTHER peers
                            let mut out_data = std::collections::HashMap::new();
                            out_data.insert("fromPeerId".to_string(), peer_id.clone());
                            out_data.insert("fromDisplayName".to_string(), sender_display_name.clone());
                            out_data.insert("text".to_string(), text.clone());

                            let broadcast_msg = Envelope {
                                event: "notification".to_string(),
                                event_data: Some(EventData {
                                    method: "chat_message".to_string(),
                                    data: out_data,
                                }),
                            };

                            let peers_guard = peers.lock().await;
                            for (id, peer) in peers_guard.iter() {
                                // Skip the sender
                                if *id != peer_id {
                                    let ctx = format!("chat_broadcast → {}", id);
                                    send_server_message(&peer.sender, &broadcast_msg, &ctx).await;
                                }
                            }
                        } else {
                            println!(
                                "[SERVER DEBUG] Unknown client method '{}', data: {:?}",
                                method, data
                            );
                        }
                    }
                    Err(e) => {
                        println!("[SERVER] ❌ Failed to decode client message: {}", e);
                    }
                }
            }

            WsMessage::Text(_) => {
                // Legacy text support - ignore or convert
                println!("[SERVER] ⚠️ Received text message (protobuf expected), ignoring");
            }

            WsMessage::Ping(payload) => {
                let mut locked = client.lock().await;
                let _ = locked.send(WsMessage::Pong(payload)).await;
            }

            WsMessage::Pong(_) => {}

            WsMessage::Close(frame) => {
                let mut locked = client.lock().await;
                let _ = locked.send(WsMessage::Close(frame)).await;
                break;
            }
        }
    }

    // Remove peer from shared state on disconnect and notify others
    {
        let mut peers_guard = peers.lock().await;
        peers_guard.remove(&peer_id);
        println!("[SERVER] Peer disconnected: {} ({})", display_name, peer_id);
        
        // Broadcast \"peer_left\" notification to all remaining peers
        let mut leave_data = std::collections::HashMap::new();
        leave_data.insert("peerId".to_string(), peer_id.clone());
        leave_data.insert("displayName".to_string(), display_name.clone());
        leave_data.insert("message".to_string(), format!("{} left", display_name));

        let leave_notification = Envelope {
            event: "notification".to_string(),
            event_data: Some(EventData {
                method: "peer_left".to_string(),
                data: leave_data,
            }),
        };
        
        for (id, peer) in peers_guard.iter() {
            let ctx = format!("leave_notification → {}", id);
            send_server_message(&peer.sender, &leave_notification, &ctx).await;
        }
    }

    println!("[SERVER] Client disconnected");
}
