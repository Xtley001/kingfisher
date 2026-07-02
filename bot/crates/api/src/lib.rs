//! # API Server
//!
//! ## Prometheus Metrics endpoint at GET /metrics
//! ## Persistence module wired at startup

#![allow(clippy::too_many_arguments)]
pub mod alerts;
pub mod auth;
pub mod metrics;
pub mod persistence;
pub mod routes;

use axum::{Router, middleware};
use tower_http::cors::CorsLayer;
use std::sync::Arc;
use tokio::sync::RwLock;
use anyhow::Result;
use kingfisher_core::state::BotState;

pub type SharedState = Arc<RwLock<BotState>>;

pub async fn start(state: SharedState) -> Result<()> {
    // Validate API_KEY at startup — an unset key allows unauthenticated access to
    // the kill switch, parameter updates, and opportunity feed. Hard-fail immediately.
    let api_key = std::env::var("API_KEY")
        .map_err(|_| anyhow::anyhow!("API_KEY environment variable is not set — refusing to start API server. Set a strong random value in .env"))?;
    if api_key.trim().is_empty() {
        anyhow::bail!("API_KEY is set but empty — refusing to start API server. Set a strong random value in .env");
    }

    let port = std::env::var("API_PORT")
        .unwrap_or_else(|_| "3001".into())
        .parse::<u16>()
        .unwrap_or(3001);

    // Restrict CORS to configured dashboard origin
    let dashboard_origin = std::env::var("DASHBOARD_ORIGIN")
        .unwrap_or_else(|_| "http://localhost:5173".into());
    let allowed_origin: axum::http::HeaderValue = dashboard_origin
        .parse()
        .expect("DASHBOARD_ORIGIN is not a valid header value");
    let cors = CorsLayer::new()
        .allow_origin(tower_http::cors::AllowOrigin::exact(allowed_origin))
        .allow_methods([
            axum::http::Method::GET,
            axum::http::Method::PATCH,
            axum::http::Method::POST,
        ])
        .allow_headers([
            axum::http::header::CONTENT_TYPE,
            axum::http::header::AUTHORIZATION,
            "x-api-key".parse().unwrap(),
        ]);

    // Protected routes — require API key
    let protected = Router::new()
        .route("/api/state",    axum::routing::get(routes::get_state))
        .route("/api/params",   axum::routing::patch(routes::update_params))
        .route("/api/command",  axum::routing::post(routes::send_command))
        .route("/api/withdraw", axum::routing::post(routes::trigger_withdrawal))
        .layer(middleware::from_fn_with_state(state.clone(), auth::require_api_key));

    // WebSocket stream
    let ws_route = Router::new()
        .route("/ws", axum::routing::get(routes::ws_handler));

    // Prometheus metrics endpoint — auth-protected.
    // Previously unauthenticated — exposed P&L, trade counts, gas costs, pool state
    // to anyone with the API URL. Prometheus scraper must send X-Api-Key header.
    // Update prometheus.yml: headers: { X-Api-Key: 'your-api-key' }
    let metrics_route = Router::new()
        .route("/metrics", axum::routing::get(routes::get_metrics))
        .layer(middleware::from_fn_with_state(state.clone(), auth::require_api_key));

    // Health — unauthenticated, uptime/systemd probe
    let health = Router::new()
        .route("/health", axum::routing::get(routes::health));

    let app = Router::new()
        .merge(protected)
        .merge(ws_route)
        .merge(metrics_route)
        .merge(health)
        .layer(cors)
        .with_state(state);

    let addr     = format!("0.0.0.0:{}", port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!(port, "API server listening");

    axum::serve(listener, app).await?;
    Ok(())
}
