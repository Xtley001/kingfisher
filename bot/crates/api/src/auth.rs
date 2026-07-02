use axum::{
    extract::{Request, State},
    middleware::Next,
    response::{IntoResponse, Response},
    http::StatusCode,
};
use crate::SharedState;

/// Middleware: require X-Api-Key header on all protected routes.
/// Constant-time comparison prevents timing attacks.
pub async fn require_api_key(
    State(_state): State<SharedState>,
    req:           Request,
    next:          Next,
) -> Response {
    let api_key = std::env::var("API_KEY").unwrap_or_default();

    if api_key.is_empty() {
        tracing::error!("API_KEY not set — all protected routes blocked");
        return (StatusCode::INTERNAL_SERVER_ERROR, "API_KEY not configured").into_response();
    }

    let provided = req
        .headers()
        .get("x-api-key")
        .or_else(|| req.headers().get("X-Api-Key"))
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    // Constant-time comparison (prevent timing oracle)
    let matches = provided.len() == api_key.len()
        && provided.bytes().zip(api_key.bytes()).fold(0u8, |acc, (a, b)| acc | (a ^ b)) == 0;

    if !matches {
        tracing::warn!(
            ip = ?req.headers().get("x-forwarded-for"),
            "Unauthorized API access attempt"
        );
        return (StatusCode::UNAUTHORIZED, "Invalid or missing API key").into_response();
    }

    next.run(req).await
}
