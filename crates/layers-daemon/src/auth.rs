//! Bearer-token authentication middleware for the gateway.

use axum::extract::Request;
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::Response;

/// Axum middleware that enforces `Authorization: Bearer <token>` on every
/// request that passes through it.  Routes that should remain public (e.g.
/// `/health`) must be mounted *outside* the middleware layer.
///
/// The expected token is carried via a request extension (`BearerToken`).
/// When `BearerToken` is `None` auth is disabled and all requests pass.
pub async fn require_bearer(req: Request, next: Next) -> Result<Response, StatusCode> {
    let expected = req.extensions().get::<BearerToken>().cloned();

    if let Some(BearerToken(Some(ref token))) = expected {
        let provided = req
            .headers()
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "));

        if provided != Some(token.as_str()) {
            return Err(StatusCode::UNAUTHORIZED);
        }
    }

    Ok(next.run(req).await)
}

/// Wrapper so we can inject the expected bearer token as a request extension.
#[derive(Clone, Debug)]
pub struct BearerToken(pub Option<String>);
