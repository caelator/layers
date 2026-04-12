//! Axum HTTP + WebSocket gateway server.
//!
//! Provides health checks, REST API, webhook ingestion, and WebSocket streaming.

use std::sync::Arc;

use axum::extract::ws::{Message as WsMessage, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use layers_channels::manager::ChannelManager;
use layers_core::{DaemonConfig, InboundMessage, PeerKind, TlsConfig};
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tower_http::cors::{Any, CorsLayer};
use tracing::{error, info};

/// Gateway server wrapping the axum router and configuration.
pub struct Gateway {
    config: GatewayConfig,
    channel_manager: Arc<ChannelManager>,
}

/// Gateway-specific configuration (extends `DaemonConfig`).
#[derive(Debug, Clone)]
pub struct GatewayConfig {
    pub bind_address: String,
    pub port: u16,
    pub tls: Option<TlsConfig>,
    pub bearer_token: Option<String>,
}

impl From<&DaemonConfig> for GatewayConfig {
    fn from(dc: &DaemonConfig) -> Self {
        Self {
            bind_address: dc.bind_address.clone(),
            port: dc.port,
            tls: dc.tls.clone(),
            bearer_token: None,
        }
    }
}

/// Shared state for axum handlers.
#[derive(Clone)]
struct AppState {
    channel_manager: Arc<ChannelManager>,
    bearer_token: Option<String>,
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

impl Gateway {
    /// Create a new gateway.
    #[must_use]
    pub fn new(config: GatewayConfig, channel_manager: Arc<ChannelManager>) -> Self {
        Self {
            config,
            channel_manager,
        }
    }

    /// Build the axum router with all routes and middleware.
    #[must_use]
    #[allow(clippy::double_must_use)]
    pub fn router(&self) -> Router {
        let state = AppState {
            channel_manager: Arc::clone(&self.channel_manager),
            bearer_token: self.config.bearer_token.clone(),
        };

        let cors = CorsLayer::new()
            .allow_origin(Any)
            .allow_methods(Any)
            .allow_headers(Any);

        Router::new()
            .route("/health", get(health_handler))
            .route("/ws", get(ws_handler))
            .route("/api/status", get(status_handler))
            .route("/api/sessions", get(sessions_handler))
            .route("/api/config", get(config_handler))
            .route("/webhook/{channel}", post(webhook_handler))
            .layer(cors)
            .with_state(state)
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

    // For webchat adapter integration, register this client if the webchat adapter is available.
    // For now, relay messages through the channel manager.
    while let Some(Ok(msg)) = socket.recv().await {
        match msg {
            WsMessage::Text(text) => {
                let inbound = InboundMessage {
                    channel: "webchat".to_string(),
                    channel_message_id: uuid::Uuid::new_v4().to_string(),
                    peer_id: client_id.clone(),
                    peer_display_name: "WebSocket User".to_string(),
                    peer_kind: PeerKind::User,
                    text: text.to_string(),
                    attachments: Vec::new(),
                    thread_id: None,
                    reply_to_message_id: None,
                    channel_metadata: None,
                    timestamp: chrono::Utc::now(),
                };
                if let Err(e) = state.channel_manager.submit_inbound(inbound).await {
                    error!(error = %e, "failed to submit ws inbound message");
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

async fn sessions_handler() -> impl IntoResponse {
    // Placeholder — will be wired to session store.
    Json(serde_json::json!({ "sessions": [] }))
}

async fn config_handler() -> impl IntoResponse {
    // Placeholder — will expose non-sensitive config.
    Json(serde_json::json!({ "config": {} }))
}

async fn webhook_handler(
    Path(channel): Path<String>,
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(payload): Json<WebhookPayload>,
) -> impl IntoResponse {
    // Bearer token auth check.
    if let Some(ref expected) = state.bearer_token {
        let provided = headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "));
        if provided != Some(expected.as_str()) {
            return StatusCode::UNAUTHORIZED;
        }
    }

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
