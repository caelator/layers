//! Axum HTTP + WebSocket gateway server.
//!
//! Provides health checks, REST API, webhook ingestion, and WebSocket streaming.

use std::convert::Infallible;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;

use axum::extract::ws::{Message as WsMessage, WebSocket, WebSocketUpgrade};
use axum::extract::{Multipart, Path, State};
use axum::http::StatusCode;
use axum::middleware;
use axum::response::IntoResponse;
use axum::response::sse::{Event, Sse};
use axum::routing::{any, delete, get, post};
use axum::{Json, Router};
use layers_channels::manager::ChannelManager;
use layers_core::{DaemonConfig, InboundMessage, PeerKind, SessionFilter, SessionStore, TlsConfig};
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tokio_stream::{StreamExt, iter};
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::{ServeDir, ServeFile};
use tracing::{error, info};

use crate::auth::{BearerToken, require_bearer};

/// Gateway server wrapping the axum router and configuration.
pub struct Gateway {
    config: GatewayConfig,
    channel_manager: Arc<ChannelManager>,
    session_store: Option<Arc<dyn SessionStore>>,
    brain_dispatcher: Option<Arc<layers_runtime::brain::BrainDispatcher>>,
}

/// Gateway-specific configuration (extends `DaemonConfig`).
#[derive(Debug, Clone)]
pub struct GatewayConfig {
    pub bind_address: String,
    pub port: u16,
    pub tls: Option<TlsConfig>,
    pub bearer_token: Option<String>,
    pub portal_dir: Option<PathBuf>,
    pub upload_dir: Option<PathBuf>,
}

impl From<&DaemonConfig> for GatewayConfig {
    fn from(dc: &DaemonConfig) -> Self {
        Self {
            bind_address: dc.bind_address.clone(),
            port: dc.port,
            tls: dc.tls.clone(),
            bearer_token: None,
            portal_dir: Some(PathBuf::from("./portal/dist")),
            upload_dir: Some(PathBuf::from("./uploads")),
        }
    }
}

/// Shared state for axum handlers.
#[derive(Clone)]
pub(crate) struct AppState {
    channel_manager: Arc<ChannelManager>,
    session_store: Option<Arc<dyn SessionStore>>,
    bind_address: String,
    port: u16,
    upload_dir: PathBuf,
    provider_configs: Vec<ProviderConfigSummary>,
    mcp_servers: Vec<McpServerSummary>,
    brain_dispatcher: Option<Arc<layers_runtime::brain::BrainDispatcher>>,
}

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
    version: &'static str,
}

#[derive(Serialize)]
struct StatusResponse {
    uptime_secs: u64,
    channels: Vec<ChannelStatus>,
}

#[derive(Serialize)]
struct ChannelStatus {
    name: String,
    health: String,
}

#[derive(Deserialize)]
struct WebhookPayload {
    #[serde(default)]
    peer_id: Option<String>,
    #[serde(default)]
    peer_display_name: Option<String>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    thread_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ChatRequest {
    message: String,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    attachments: Vec<ChatAttachment>,
}

#[derive(Debug, Deserialize)]
struct ChatAttachment {
    #[allow(dead_code)]
    file_id: String,
}

#[derive(Clone, Serialize)]
struct ProviderConfigSummary {
    name: String,
    api_base: Option<String>,
    has_api_key: bool,
    models: Vec<String>,
}

#[derive(Clone, Serialize)]
struct McpServerSummary {
    name: String,
    url: Option<String>,
    has_api_key: bool,
    tools: Vec<String>,
}

impl Gateway {
    /// Create a new gateway.
    #[must_use]
    pub fn new(config: GatewayConfig, channel_manager: Arc<ChannelManager>) -> Self {
        Self {
            config,
            channel_manager,
            session_store: None,
            brain_dispatcher: None,
        }
    }

    /// Attach a session store for the `/api/sessions` endpoint.
    #[must_use]
    pub fn with_session_store(mut self, store: Arc<dyn SessionStore>) -> Self {
        self.session_store = Some(store);
        self
    }

    /// Attach a brain dispatcher for real AI model responses.
    #[must_use]
    pub fn with_brain_dispatcher(
        mut self,
        dispatcher: Arc<layers_runtime::brain::BrainDispatcher>,
    ) -> Self {
        self.brain_dispatcher = Some(dispatcher);
        self
    }

    /// Build the axum router with all routes and middleware.
    #[must_use]
    #[allow(clippy::double_must_use)]
    pub fn router(&self) -> Router {
        let upload_dir = self
            .config
            .upload_dir
            .clone()
            .unwrap_or_else(|| PathBuf::from("./uploads"));
        // Ensure upload directory exists.
        let _ = std::fs::create_dir_all(&upload_dir);

        let state = AppState {
            channel_manager: Arc::clone(&self.channel_manager),
            session_store: self.session_store.clone(),
            bind_address: self.config.bind_address.clone(),
            port: self.config.port,
            upload_dir: upload_dir.clone(),
            provider_configs: Vec::new(),
            mcp_servers: Vec::new(),
            brain_dispatcher: self.brain_dispatcher.clone(),
        };

        let cors = CorsLayer::new()
            .allow_origin(Any)
            .allow_methods(Any)
            .allow_headers(Any);

        let bearer = BearerToken(self.config.bearer_token.clone());

        // Protected routes — require bearer auth when a token is configured.
        // Layer order: outer layers process the request first.
        // Extension must be outer so the token is available when require_bearer runs.
        let protected_uploads = ServeDir::new(upload_dir);

        let protected = Router::new()
            .route("/ws", get(ws_handler))
            .route("/api/status", get(status_handler))
            .route("/api/chat", post(chat_handler))
            .route("/api/sessions", get(sessions_handler))
            .route("/api/sessions", post(create_session_handler))
            .route("/api/sessions/{id}", delete(delete_session_handler))
            .route("/api/models", get(models_handler))
            .route("/api/upload", post(upload_handler))
            .route("/api/config", get(config_handler))
            .route("/api/config/providers", get(config_providers_handler))
            .route("/api/config/mcp", get(config_mcp_handler))
            .route("/api/daemon/restart", post(restart_handler))
            .nest_service("/api/uploads", protected_uploads)
            .route("/api/{*path}", any(api_not_found_handler))
            .route("/webhook/{channel}", post(webhook_handler))
            .layer(middleware::from_fn(require_bearer))
            .layer(axum::Extension(bearer));

        // Build the base router.
        let mut app = Router::new()
            .route("/health", get(health_handler))
            .merge(protected)
            .layer(cors)
            .with_state(state);

        // Serve portal static files if configured.
        if let Some(ref portal_dir) = self.config.portal_dir {
            if portal_dir.exists() {
                info!(dir = %portal_dir.display(), "serving portal static files");
                let serve = ServeDir::new(portal_dir)
                    .append_index_html_on_directories(true)
                    .not_found_service(ServeFile::new(portal_dir.join("index.html")));
                app = app.fallback_service(serve);
            }
        }

        app
    }

    /// Start serving. This blocks until the server shuts down.
    ///
    /// # Errors
    /// Returns an error if binding or serving fails.
    pub async fn serve(&self) -> layers_core::Result<()> {
        let addr = format!("{}:{}", self.config.bind_address, self.config.port);
        info!(addr = %addr, "gateway listening");

        // TLS placeholder — log if configured but not yet wired.
        if self.config.tls.is_some() {
            info!("TLS configured but not yet wired — serving plain HTTP");
        }

        let listener = TcpListener::bind(&addr)
            .await
            .map_err(|e| layers_core::LayersError::Channel(format!("bind failed: {e}")))?;

        let router = self.router();
        axum::serve(listener, router)
            .await
            .map_err(|e| layers_core::LayersError::Channel(format!("serve failed: {e}")))?;

        Ok(())
    }

    /// Bind address string.
    #[must_use]
    pub fn bind_address(&self) -> String {
        format!("{}:{}", self.config.bind_address, self.config.port)
    }
}

// --- Handlers ---

async fn health_handler() -> impl IntoResponse {
    Json(HealthResponse {
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
    })
}

async fn ws_handler(ws: WebSocketUpgrade, State(state): State<AppState>) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_ws_connection(socket, state))
}

async fn handle_ws_connection(mut socket: WebSocket, state: AppState) {
    let client_id = uuid::Uuid::new_v4().to_string();
    info!(client_id = %client_id, "websocket client connected");

    while let Some(Ok(msg)) = socket.recv().await {
        match msg {
            WsMessage::Text(text) => {
                // Try to parse as JSON for structured messages
                let (prompt, model, session_id) =
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
                        let p = json
                            .get("message")
                            .and_then(|v| v.as_str())
                            .unwrap_or(&text)
                            .to_string();
                        let m = json
                            .get("model")
                            .and_then(|v| v.as_str())
                            .unwrap_or("default")
                            .to_string();
                        let s = json
                            .get("session_id")
                            .and_then(|v| v.as_str())
                            .map(String::from);
                        (p, m, s)
                    } else {
                        (text.to_string(), "default".to_string(), None)
                    };

                // Dispatch to brain if available
                if let Some(ref dispatcher) = state.brain_dispatcher {
                    let brain_name = if model == "default" {
                        dispatcher
                            .available_brains()
                            .first()
                            .cloned()
                            .unwrap_or_else(|| "default".to_string())
                    } else {
                        model
                    };
                    let sid = session_id.unwrap_or_else(|| client_id.clone());

                    match dispatcher.dispatch(&brain_name, &prompt, Some(&sid)).await {
                        Ok(mut stream) => {
                            while let Some(event) = stream.next().await {
                                let json = match &event {
                                    layers_core::types::BrainEvent::Token { content } => {
                                        serde_json::json!({"type": "token", "content": content})
                                    }
                                    layers_core::types::BrainEvent::Done { session_id } => {
                                        serde_json::json!({"type": "done", "session_id": session_id})
                                    }
                                    layers_core::types::BrainEvent::Error { message } => {
                                        serde_json::json!({"type": "error", "message": message})
                                    }
                                };
                                if socket
                                    .send(WsMessage::Text(json.to_string().into()))
                                    .await
                                    .is_err()
                                {
                                    break;
                                }
                            }
                        }
                        Err(e) => {
                            let err =
                                serde_json::json!({"type": "error", "message": e.to_string()});
                            let _ = socket.send(WsMessage::Text(err.to_string().into())).await;
                        }
                    }
                } else {
                    // No brain — echo back
                    let response = serde_json::json!({"type": "token", "content": "No brain configured. Configure in layers.toml."});
                    let _ = socket
                        .send(WsMessage::Text(response.to_string().into()))
                        .await;
                    let done = serde_json::json!({"type": "done", "session_id": client_id});
                    let _ = socket.send(WsMessage::Text(done.to_string().into())).await;
                }
            }
            WsMessage::Close(_) => break,
            _ => {}
        }
    }

    info!(client_id = %client_id, "websocket client disconnected");
}

async fn status_handler(State(state): State<AppState>) -> impl IntoResponse {
    let handles = state.channel_manager.health_all().await;
    let channels = handles
        .into_iter()
        .map(|h| ChannelStatus {
            name: h.name,
            health: format!("{:?}", h.health),
        })
        .collect();

    Json(StatusResponse {
        uptime_secs: 0, // Placeholder — lifecycle tracker will provide real uptime.
        channels,
    })
}

async fn sessions_handler(State(state): State<AppState>) -> impl IntoResponse {
    let Some(ref store) = state.session_store else {
        return Json(serde_json::json!({ "sessions": [] }));
    };

    let filter = SessionFilter {
        agent_id: None,
        channel: None,
        peer_id: None,
        since: None,
    };

    match store.list(&filter).await {
        Ok(sessions) => Json(serde_json::json!({ "sessions": sessions })),
        Err(e) => {
            error!(error = %e, "failed to list sessions");
            Json(serde_json::json!({ "sessions": [], "error": e.to_string() }))
        }
    }
}

async fn config_handler(State(state): State<AppState>) -> impl IntoResponse {
    let providers = state
        .provider_configs
        .iter()
        .map(|provider| provider.name.clone())
        .collect::<Vec<_>>();
    let mcp_servers = state
        .mcp_servers
        .iter()
        .map(|server| server.name.clone())
        .collect::<Vec<_>>();

    Json(serde_json::json!({
        "daemon": {
            "bind_address": state.bind_address,
            "port": state.port,
        },
        "providers": providers,
        "mcp_servers": mcp_servers,
    }))
}

async fn config_providers_handler(State(state): State<AppState>) -> impl IntoResponse {
    Json(serde_json::json!({
        "providers": state.provider_configs,
    }))
}

async fn config_mcp_handler(State(state): State<AppState>) -> impl IntoResponse {
    Json(serde_json::json!({
        "mcp_servers": state.mcp_servers,
    }))
}

async fn restart_handler() -> impl IntoResponse {
    Json(serde_json::json!({
        "restarting": true,
    }))
}

async fn webhook_handler(
    Path(channel): Path<String>,
    State(state): State<AppState>,
    Json(payload): Json<WebhookPayload>,
) -> impl IntoResponse {
    let text = match payload.text {
        Some(t) => t,
        None => return StatusCode::BAD_REQUEST,
    };

    let msg = InboundMessage {
        channel,
        channel_message_id: uuid::Uuid::new_v4().to_string(),
        peer_id: payload.peer_id.unwrap_or_else(|| "webhook".to_string()),
        peer_display_name: payload
            .peer_display_name
            .unwrap_or_else(|| "Webhook".to_string()),
        peer_kind: PeerKind::System,
        text,
        attachments: Vec::new(),
        thread_id: payload.thread_id,
        reply_to_message_id: None,
        channel_metadata: None,
        timestamp: chrono::Utc::now(),
    };

    if let Err(e) = state.channel_manager.submit_inbound(msg).await {
        error!(error = %e, "webhook submit failed");
        return StatusCode::INTERNAL_SERVER_ERROR;
    }

    StatusCode::OK
}

// --- Portal API handlers ---

async fn chat_handler(
    State(state): State<AppState>,
    Json(payload): Json<ChatRequest>,
) -> Sse<Pin<Box<dyn tokio_stream::Stream<Item = Result<Event, Infallible>> + Send>>> {
    let session_id = payload
        .session_id
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let model = payload.model.unwrap_or_else(|| "default".to_string());

    // If brain dispatcher is available, use real model
    if let Some(ref dispatcher) = state.brain_dispatcher {
        let brain_name = if model == "default" {
            dispatcher
                .available_brains()
                .first()
                .cloned()
                .unwrap_or_else(|| "default".to_string())
        } else {
            model.clone()
        };

        match dispatcher
            .dispatch(&brain_name, &payload.message, Some(&session_id))
            .await
        {
            Ok(stream) => {
                let event_stream = stream.filter_map(move |event| {
                    let json = match &event {
                        layers_core::types::BrainEvent::Token { content } => {
                            serde_json::json!({"type": "token", "content": content})
                        }
                        layers_core::types::BrainEvent::Done { session_id } => {
                            serde_json::json!({"type": "done", "session_id": session_id})
                        }
                        layers_core::types::BrainEvent::Error { message } => {
                            serde_json::json!({"type": "error", "message": message})
                        }
                    };
                    Some(Ok(Event::default()
                        .json_data(json)
                        .expect("event should serialize")))
                });
                return Sse::new(Box::pin(event_stream));
            }
            Err(e) => {
                let err_event = Ok(Event::default()
                    .json_data(serde_json::json!({"type": "error", "message": e.to_string()}))
                    .expect("error event should serialize"));
                return Sse::new(Box::pin(iter(vec![err_event])));
            }
        }
    }

    // Fallback: placeholder response
    let placeholder = format!(
        "No brain configured. Message received ({} chars). Configure a brain in layers.toml.",
        payload.message.chars().count()
    );
    let events = vec![
        Ok(Event::default()
            .json_data(serde_json::json!({"type": "token", "content": placeholder}))
            .expect("token event should serialize")),
        Ok(Event::default()
            .json_data(serde_json::json!({"type": "done", "session_id": session_id}))
            .expect("done event should serialize")),
    ];
    Sse::new(Box::pin(iter(events)))
}

async fn create_session_handler(State(state): State<AppState>) -> impl IntoResponse {
    let id = uuid::Uuid::new_v4().to_string();
    let created_at = chrono::Utc::now();

    if let Some(ref store) = state.session_store {
        let session = layers_core::Session {
            id: id.clone(),
            agent_id: "default".to_string(),
            dm_scope: None,
            thread_binding: None,
            created_at,
            updated_at: created_at,
            model: None,
            metadata: std::collections::HashMap::new(),
            message_count: 0,
            token_count: 0,
        };

        if let Err(e) = store.put(&session).await {
            error!(error = %e, "failed to persist placeholder session");
        }
    }

    Json(serde_json::json!({
        "id": id,
        "created_at": created_at,
    }))
}

async fn delete_session_handler(
    Path(id): Path<String>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    if let Some(ref store) = state.session_store {
        if let Err(e) = store.delete(&id).await {
            error!(error = %e, "failed to delete session");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "deleted": false })),
            )
                .into_response();
        }
    }

    (StatusCode::OK, Json(serde_json::json!({ "deleted": true }))).into_response()
}

async fn models_handler(State(state): State<AppState>) -> impl IntoResponse {
    let models = if let Some(ref dispatcher) = state.brain_dispatcher {
        dispatcher.available_brains().iter().map(|name| {
            serde_json::json!({ "id": name, "name": name.to_uppercase(), "provider": "cli" })
        }).collect()
    } else {
        vec![serde_json::json!({ "id": "default", "name": "Default", "provider": "layers" })]
    };
    Json(serde_json::json!({ "models": models }))
}

async fn upload_handler(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> impl IntoResponse {
    let Ok(Some(field)) = multipart.next_field().await else {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "no file provided" })),
        );
    };

    let file_name = field
        .file_name()
        .and_then(|name| {
            std::path::Path::new(name)
                .file_name()
                .map(|file_name| file_name.to_string_lossy().to_string())
        })
        .unwrap_or_else(|| "upload".to_string());
    let file_id = uuid::Uuid::new_v4().to_string();
    let content_type = field
        .content_type()
        .unwrap_or("application/octet-stream")
        .to_string();
    let ext = file_name.rsplit('.').next().unwrap_or("bin");
    let stored_name = format!("{file_id}.{ext}");
    let path = state.upload_dir.join(&stored_name);

    match field.bytes().await {
        Ok(data) => {
            if let Err(e) = tokio::fs::create_dir_all(&state.upload_dir).await {
                error!(error = %e, "failed to create upload directory");
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": "upload directory unavailable" })),
                );
            }

            if let Err(e) = tokio::fs::write(&path, &data).await {
                error!(error = %e, "failed to write upload");
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": "write failed" })),
                );
            }

            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "file_id": file_id,
                    "url": format!("/api/uploads/{stored_name}"),
                    "filename": file_name,
                    "size": data.len(),
                    "content_type": content_type,
                })),
            )
        }
        Err(e) => {
            error!(error = %e, "failed to read upload");
            (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "read failed" })),
            )
        }
    }
}

async fn api_not_found_handler() -> impl IntoResponse {
    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({ "error": "not found" })),
    )
}
