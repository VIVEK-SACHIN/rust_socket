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

    // Broadcast "peer joined" notification to all OTHER peers (not the new peer)
    let join_notification = ServerMessage {
        method: ServerMethod::System as i32,
        payload: Some(server_message::Payload::Notification(Notification {
            event: NotificationEvent::PeerJoined as i32,
            peer_id: peer_id.clone(),
            display_name: display_name.clone(),
            message: format!("{} joined", display_name),
        })),
    };

    {
        let peers_guard = peers.lock().await;
        for (id, peer) in peers_guard.iter() {
            // Skip the newly joined peer - only notify others
            if *id != peer_id {
                let mut sender_lock = peer.sender.lock().await;
                let bytes = join_notification.encode_to_vec();
                match sender_lock.send(WsMessage::Binary(bytes.into())).await {
                    Ok(_) => {
                        println!("[SERVER] ✅ Notified peer {} about {} joining", id, display_name);
                    }
                    Err(e) => {
                        println!("[SERVER] ❌ Failed to notify peer {}: {}", id, e);
                    }
                }
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
                // Parse protobuf message from client
                match ClientMessage::decode(data.as_ref()) {
                    Ok(client_msg) => {
                        println!("[SERVER DEBUG] Decoded client message - display_name: '{}', payload: {:?}", 
                            client_msg.display_name, 
                            client_msg.payload.as_ref().map(|p| format!("{:?}", p))
                        );
                        
                        let mut sender_display_name = display_name.clone();
                        let message_content: String;

                        // Extract display name if provided
                        if !client_msg.display_name.is_empty() {
                            sender_display_name = client_msg.display_name.clone();
                            // Update in peers map
                            let mut peers_guard = peers.lock().await;
                            if let Some(peer) = peers_guard.get_mut(&peer_id) {
                                peer.display_name = sender_display_name.clone();
                            }
                        }

                        // Extract message content based on payload type
                        message_content = match &client_msg.payload {
                            Some(client_message::Payload::TextMessage(text)) => {
                                println!("[SERVER DEBUG] Found TextMessage: '{}'", text);
                                text.clone()
                            }
                            Some(client_message::Payload::BinaryData(bytes)) => {
                                format!("[Binary data: {} bytes]", bytes.len())
                            }
                            Some(client_message::Payload::DataObject(obj)) => {
                                format!("[Object with {} fields]", obj.fields.len())
                            }
                            Some(client_message::Payload::DataArray(arr)) => {
                                format!("[Array with {} items]", arr.items.len())
                            }
                            None => {
                                println!("[SERVER DEBUG] ⚠️ Payload is None - message might not be encoded correctly");
                                String::from("[Empty message]")
                            }
                        };

                        println!("Received from {} ({}): {}", sender_display_name, peer_id, message_content);

                        // Broadcast message to all OTHER peers (not the sender)
                        let broadcast_msg = ServerMessage {
                            method: ServerMethod::Message as i32,
                            payload: Some(server_message::Payload::PeerMessage(PeerMessage {
                                message: message_content.clone(),
                                from_peer_id: peer_id.clone(),
                                from_display_name: sender_display_name.clone(),
                                content: Some(peer_message::Content::Text(message_content.clone())),
                            })),
                        };

                        let peers_guard = peers.lock().await;
                        let bytes = broadcast_msg.encode_to_vec();
                        for (id, peer) in peers_guard.iter() {
                            // Skip the sender
                            if *id != peer_id {
                                let mut sender_lock = peer.sender.lock().await;
                                let _ = sender_lock.send(WsMessage::Binary(bytes.clone().into())).await;
                            }
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
        
        // Broadcast "peer left" notification to all remaining peers
        let leave_notification = ServerMessage {
            method: ServerMethod::Notification as i32,
            payload: Some(server_message::Payload::Notification(Notification {
                event: NotificationEvent::PeerLeft as i32,
                peer_id: peer_id.clone(),
                display_name: display_name.clone(),
                message: format!("{} left", display_name),
            })),
        };
        
        let bytes = leave_notification.encode_to_vec();
        for (_id, peer) in peers_guard.iter() {
            let mut sender_lock = peer.sender.lock().await;
            let _ = sender_lock.send(WsMessage::Binary(bytes.clone().into())).await;
        }
    }

    println!("[SERVER] Client disconnected");
}
