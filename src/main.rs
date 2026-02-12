mod http_server;
mod ws_server;

#[tokio::main]
async fn main() {
    // Run the full HTTP + WebSocket server
    http_server::run().await;
}

