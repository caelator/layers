//! Integration tests for the gateway HTTP routes and auth middleware.

use std::sync::Arc;

use axum::body::Body;
use http_body_util::BodyExt;
use hyper::Request;
use layers_channels::manager::ChannelManager;
use layers_core::{
    Message, Result, Session, SessionFilter, SessionStore, SessionTransaction,
};
use layers_daemon::gateway::{Gateway, GatewayConfig};
use tower::ServiceExt; // for `oneshot`

// ---------------------------------------------------------------------------
// Mock SessionStore
// ---------------------------------------------------------------------------

struct MockSessionStore;

#[async_trait::async_trait]
impl SessionStore for MockSessionStore {
    async fn get(&self, _session_id: &str) -> Result<Session> {
        Err(layers_core::LayersError::SessionNotFound("not found".into()))
    }
    async fn put(&self, _session: &Session) -> Result<()> {
        Ok(())
    }
    async fn list(&self, _filter: &SessionFilter) -> Result<Vec<Session>> {
        Ok(vec![])
    }
    async fn delete(&self, _session_id: &str) -> Result<()> {
        Ok(())
    }
    async fn append_message(&self, _session_id: &str, _message: Message) -> Result<()> {
        Ok(())
    }
    async fn get_messages(&self, _session_id: &str, _limit: Option<usize>) -> Result<Vec<Message>> {
        Ok(vec![])
    }
    async fn update_model(&self, _session_id: &str, _model: &str) -> Result<()> {
        Ok(())
    }
    async fn begin_session_tx(
        &self,
        _session_id: &str,
    ) -> Result<Box<dyn SessionTransaction>> {
        Err(layers_core::LayersError::SessionNotFound("not implemented".into()))
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a gateway router with a bearer token and mock session store.
fn app_with_auth(token: &str) -> axum::Router {
    let (cm, _rx) = ChannelManager::new(16, 0);
    let config = GatewayConfig {
        bind_address: "127.0.0.1".into(),
        port: 0,
        tls: None,
        bearer_token: Some(token.to_string()),
    };
    Gateway::new(config, Arc::new(cm))
        .with_session_store(Arc::new(MockSessionStore))
        .router()
}

/// Build a gateway router with NO bearer token (auth disabled).
fn app_no_auth() -> axum::Router {
    let (cm, _rx) = ChannelManager::new(16, 0);
    let config = GatewayConfig {
        bind_address: "127.0.0.1".into(),
        port: 0,
        tls: None,
        bearer_token: None,
    };
    Gateway::new(config, Arc::new(cm))
        .with_session_store(Arc::new(MockSessionStore))
        .router()
}

async fn body_json(body: Body) -> serde_json::Value {
    let bytes = body.collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn health_returns_200_without_auth() {
    let app = app_with_auth("secret");
    let req = Request::get("/health").body(Body::empty()).unwrap();
    let resp = app.oneshot(req).await.unwrap();

    assert_eq!(resp.status(), 200);
    let json = body_json(resp.into_body()).await;
    assert_eq!(json["status"], "ok");
}

#[tokio::test]
async fn sessions_returns_401_without_bearer() {
    let app = app_with_auth("secret");
    let req = Request::get("/api/sessions").body(Body::empty()).unwrap();
    let resp = app.oneshot(req).await.unwrap();

    assert_eq!(resp.status(), 401);
}

#[tokio::test]
async fn sessions_returns_200_with_valid_bearer() {
    let app = app_with_auth("secret");
    let req = Request::get("/api/sessions")
        .header("authorization", "Bearer secret")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();

    assert_eq!(resp.status(), 200);
    let json = body_json(resp.into_body()).await;
    assert!(json["sessions"].is_array());
}

#[tokio::test]
async fn sessions_returns_401_with_wrong_bearer() {
    let app = app_with_auth("secret");
    let req = Request::get("/api/sessions")
        .header("authorization", "Bearer wrong")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();

    assert_eq!(resp.status(), 401);
}

#[tokio::test]
async fn sessions_returns_200_when_no_auth_configured() {
    let app = app_no_auth();
    let req = Request::get("/api/sessions").body(Body::empty()).unwrap();
    let resp = app.oneshot(req).await.unwrap();

    assert_eq!(resp.status(), 200);
}

#[tokio::test]
async fn webhook_rejects_without_auth() {
    let app = app_with_auth("secret");
    let req = Request::post("/webhook/test")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"text":"hello"}"#))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();

    assert_eq!(resp.status(), 401);
}

#[tokio::test]
async fn webhook_accepts_with_valid_auth() {
    let app = app_with_auth("secret");
    let req = Request::post("/webhook/test")
        .header("content-type", "application/json")
        .header("authorization", "Bearer secret")
        .body(Body::from(r#"{"text":"hello"}"#))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();

    // Auth passes (not 401). The handler may return 500 if no channel adapter
    // is registered for "test", but the important thing is auth succeeded.
    assert_ne!(resp.status(), 401);
}

#[tokio::test]
async fn webhook_returns_400_without_text() {
    let app = app_with_auth("secret");
    let req = Request::post("/webhook/test")
        .header("content-type", "application/json")
        .header("authorization", "Bearer secret")
        .body(Body::from(r#"{}"#))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();

    assert_eq!(resp.status(), 400);
}

#[tokio::test]
async fn cors_headers_present() {
    let app = app_with_auth("secret");
    let req = Request::get("/health")
        .header("origin", "http://example.com")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();

    assert_eq!(resp.status(), 200);
    assert!(resp.headers().contains_key("access-control-allow-origin"));
}

#[tokio::test]
async fn status_returns_200_with_auth() {
    let app = app_with_auth("secret");
    let req = Request::get("/api/status")
        .header("authorization", "Bearer secret")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();

    assert_eq!(resp.status(), 200);
    let json = body_json(resp.into_body()).await;
    assert!(json["channels"].is_array());
}

#[tokio::test]
async fn status_returns_401_without_auth() {
    let app = app_with_auth("secret");
    let req = Request::get("/api/status").body(Body::empty()).unwrap();
    let resp = app.oneshot(req).await.unwrap();

    assert_eq!(resp.status(), 401);
}
