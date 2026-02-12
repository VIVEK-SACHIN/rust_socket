use axum::{body::Body, http::Request, middleware::{self, Next}, routing::{get, post}, Json, Router};
use http_body_util::BodyExt;
use std::net::SocketAddr;
use std::time::{SystemTime, UNIX_EPOCH};

pub async fn run() {
    // Router for all /api/* paths. Middleware here only affects /api routes.
    let api_router = Router::new()
        .route("/ping", get(ping))
        .route("/time", get(time))
        .route("/echo-json", post(echo_json))
        .layer(middleware::from_fn(log_requests));

    // Top-level router. No global middleware; only /api/* goes through log_requests.
    let app = Router::new()
        .route("/", get(root))
        .route("/health", get(health))
        .nest("/api", api_router)
        .route("/echo", post(echo))
        .route("/ws", get(crate::ws_server::ws_handler));

    let addr = SocketAddr::from(([127, 0, 0, 1], 7878));
    println!("HTTP server running on http://{addr}");

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn root() -> &'static str {
    "Rust server is running.\nTry:\n- GET /health\n- GET /api/ping\n- GET /api/time\n- POST /api/echo-json\n- POST /echo\n- WS /ws\n"
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "status": "ok" }))
}

async fn ping() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "message": "pong" }))
}

async fn time() -> Json<serde_json::Value> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    Json(serde_json::json!({ "unix": now }))
}

async fn echo_json(Json(payload): Json<serde_json::Value>) -> Json<serde_json::Value> {
    println!("echo_json function called");
    println!("Echoing JSON: {:?}", payload);
    println!("Echoing JSON: {:?}", Json(payload.clone()));
    Json(payload)
}

async fn echo(body: String) -> String {
    body
}

async fn log_requests(mut req: Request<Body>, next: Next) -> axum::response::Response {
    println!("log request function called");

    let uri = req.uri().clone();
    let method = req.method().clone();

    let body_bytes = req
        .body_mut()
        .collect()
        .await
        .map(|data| data.to_bytes())
        .unwrap_or_default();

    let body_text = String::from_utf8_lossy(&body_bytes);
    let query = uri.query().unwrap_or("");
    println!(
        "Request: {} {}{}{}",
        method,
        uri.path(),
        if query.is_empty() { "" } else { "?" },
        query
    );
    println!("Body: {}", body_text);

    *req.body_mut() = Body::from(body_bytes);
    next.run(req).await
}

