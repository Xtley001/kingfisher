use axum::{
    extract::{State, WebSocketUpgrade},
    extract::ws::{WebSocket, Message},
    response::IntoResponse,
    Json,
    http::StatusCode,
};
use futures::{SinkExt, StreamExt};
use serde::Deserialize;
use tokio::time::{interval, Duration};

use kingfisher_core::config::{FlashSourcePreference, SlippageModelParams};
use crate::SharedState;

// ─── Health ──────────────────────────────────────────────────────────────────

/// GET /health — unauthenticated, systemd/uptime health check
pub async fn health() -> &'static str {
    "ok"
}

// ─── State snapshot ──────────────────────────────────────────────────────────

/// GET /api/state — full BotState JSON snapshot
pub async fn get_state(State(state): State<SharedState>) -> impl IntoResponse {
    let s = state.read().await;
    Json(s.clone())
}

// ─── Parameter updates ───────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct ParamUpdate {
    pub min_profit_usd:                 Option<f64>,
    pub min_gas_roi:                    Option<f64>,  // dynamic profit floor multiplier
    pub min_imbalance_pct:              Option<f64>,
    pub min_velocity:                   Option<f64>,
    pub gas_reserve_eth:                Option<f64>,
    pub alert_gas_eth:                  Option<f64>,
    pub abs_cap_usd:                    Option<f64>,
    pub gas_limit_override:             Option<u64>,
    pub timeboost_min_profit_usd:       Option<f64>,
    pub timeboost_race_loss_threshold:  Option<f64>,
    pub stress_priority_fee_multiplier: Option<f64>,
    pub gas_limit_2hop:                 Option<u64>,
    pub gas_limit_4hop:                 Option<u64>,
    pub calldata_cache_enabled:         Option<bool>,
    pub presigned_pool_enabled:         Option<bool>,
    pub flash_source_preference:        Option<FlashSourcePreference>,
    pub slippage_model:                 Option<SlippageModelParams>,
}

/// PATCH /api/params — partial BotParams update, applied live, persisted to .env
pub async fn update_params(
    State(state): State<SharedState>,
    Json(body): Json<ParamUpdate>,
) -> impl IntoResponse {
    let mut s = state.write().await;

    // validate bounds before applying — prevents setting profit floor to 0 or negative,
    // disabling the gas halt, or removing the flash loan ceiling.
    if let Some(v) = body.min_profit_usd {
        // minimum is $1 (absolute safety net). Dynamic floor is gas * min_gas_roi.
        if v < 1.0 {
            return (StatusCode::BAD_REQUEST, "min_profit_usd must be >= 1.0 (absolute floor; dynamic floor = max(this, gas * min_gas_roi))").into_response();
        }
        s.params.min_profit_usd = v;
    }
    if let Some(v) = body.min_gas_roi {
        // minimum 300% ROI on gas. Range [1.0, 20.0] — below 1.0 accepts negative-ROI trades.
        if !v.is_finite() || !(1.0..=20.0).contains(&v) {
            return (StatusCode::BAD_REQUEST, "min_gas_roi must be in [1.0, 20.0]").into_response();
        }
        s.params.min_gas_roi = v;
    }
    if let Some(v) = body.min_imbalance_pct {
        if !(0.1..=50.0).contains(&v) {
            return (StatusCode::BAD_REQUEST, "min_imbalance_pct must be in [0.1, 50.0]").into_response();
        }
        s.params.min_imbalance_pct = v;
    }
    if let Some(v) = body.min_velocity {
        if v < 0.0 {
            return (StatusCode::BAD_REQUEST, "min_velocity must be >= 0.0").into_response();
        }
        s.params.min_velocity = v;
    }
    if let Some(v) = body.gas_reserve_eth {
        if v < 0.01 {
            return (StatusCode::BAD_REQUEST, "gas_reserve_eth must be >= 0.01").into_response();
        }
        s.params.gas_reserve_eth = v;
    }
    if let Some(v) = body.alert_gas_eth {
        if v < 0.0 {
            return (StatusCode::BAD_REQUEST, "alert_gas_eth must be >= 0.0").into_response();
        }
        s.params.alert_gas_eth = v;
    }
    if let Some(v) = body.abs_cap_usd {
        if !v.is_finite() || !(1.0..=25_000_000.0).contains(&v) {
            return (StatusCode::BAD_REQUEST, "abs_cap_usd must be in [1.0, 25_000_000]").into_response();
        }
        if v > 10_000_000.0 {
            tracing::warn!(abs_cap_usd = v, "⚠️ High abs_cap_usd requested (> $10M ceiling)");
        }
        s.params.abs_cap_usd = v;
    }
    if let Some(v) = body.gas_limit_override {
        if !(200_000..=5_000_000).contains(&v) {
            return (StatusCode::BAD_REQUEST, "gas_limit_override must be in [200_000, 5_000_000]").into_response();
        }
        s.params.gas_limit_override = v;
    }
    if let Some(v) = body.timeboost_min_profit_usd {
        if !v.is_finite() || v < 0.0 {
            return (StatusCode::BAD_REQUEST, "timeboost_min_profit_usd must be >= 0.0").into_response();
        }
        s.params.timeboost_min_profit_usd = v;
    }
    if let Some(v) = body.timeboost_race_loss_threshold {
        if !v.is_finite() || !(0.0..=1.0).contains(&v) {
            return (StatusCode::BAD_REQUEST, "timeboost_race_loss_threshold must be in [0.0, 1.0]").into_response();
        }
        s.params.timeboost_race_loss_threshold = v;
    }
    if let Some(v) = body.stress_priority_fee_multiplier {
        if !v.is_finite() || !(0.0..=2.0).contains(&v) {
            return (StatusCode::BAD_REQUEST, "stress_priority_fee_multiplier must be in [0.0, 2.0]").into_response();
        }
        s.params.stress_priority_fee_multiplier = v;
    }
    if let Some(v) = body.gas_limit_2hop {
        if !(200_000..=2_000_000).contains(&v) {
            return (StatusCode::BAD_REQUEST, "gas_limit_2hop must be in [200_000, 2_000_000]").into_response();
        }
        s.params.gas_limit_2hop = v;
    }
    if let Some(v) = body.gas_limit_4hop {
        if !(400_000..=5_000_000).contains(&v) {
            return (StatusCode::BAD_REQUEST, "gas_limit_4hop must be in [400_000, 5_000_000]").into_response();
        }
        s.params.gas_limit_4hop = v;
    }
    if let Some(v) = body.calldata_cache_enabled {
        s.params.calldata_cache_enabled = v;
    }
    if let Some(v) = body.presigned_pool_enabled {
        s.params.presigned_pool_enabled = v;
    }
    if let Some(v) = body.flash_source_preference {
        s.params.flash_source_preference = v;
    }
    if let Some(v) = body.slippage_model {
        if v.hard_cap > 0.03 || v.hard_cap <= 0.0 {
            return (StatusCode::BAD_REQUEST, "slippage_model.hard_cap cannot exceed 0.03 (3% safety ceiling)").into_response();
        }
        if v.depth_base < 0.0 || v.depth_shallow < 0.0 || v.time_drift_rate < 0.0 || v.time_drift_cap < 0.0 || v.size_ratio_weight < 0.0 {
            return (StatusCode::BAD_REQUEST, "slippage_model weights must be non-negative").into_response();
        }
        s.params.slippage_model = v;
    }

    tracing::info!(params = ?s.params, "Parameters updated via dashboard");

    // persist so params survive restarts — return 500 if write fails so
    // operator knows the in-memory update is not durable.
    if let Err(e) = persist_params_to_env(&s.params) {
        tracing::error!(error = %e, "Failed to persist params to env file");
        return (StatusCode::INTERNAL_SERVER_ERROR,
            format!("In-memory update applied but persistence failed: {}", e))
            .into_response();
    }

    StatusCode::OK.into_response()
}

fn persist_params_to_env(params: &kingfisher_core::config::BotParams) -> Result<(), String> {
    // Persist to {KINGFISHER_DATA_DIR}/params.json (default /var/lib/kingfisher on bare
    // metal). This file is read at startup in BotParams::from_env() and takes priority
    // over env vars, so live parameter changes survive restarts. If the directory is not
    // writable, the write fails gracefully — the in-memory update still applies.
    let dir = kingfisher_core::config::data_dir();
    let params_path = format!("{}/params.json", dir);
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("Cannot create {} directory: {} — params will not survive restart", dir, e))?;

    let json = serde_json::to_string_pretty(params)
        .map_err(|e| format!("Cannot serialise params to JSON: {}", e))?;

    std::fs::write(&params_path, json)
        .map_err(|e| format!("Cannot write {}: {} — in-memory update applied but will NOT survive restart", params_path, e))?;

    tracing::info!(path = %params_path, "Parameters persisted to disk");
    Ok(())
}

// ─── Commands ────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CommandBody {
    pub command: String,
}

/// POST /api/command — pause | resume
pub async fn send_command(
    State(state): State<SharedState>,
    Json(body): Json<CommandBody>,
) -> impl IntoResponse {
    let cmd = body.command.as_str().to_owned();

    match cmd.as_str() {
        "pause" => {
            state.write().await.running = false;
            tracing::warn!("Bot PAUSED via dashboard");
            crate::alerts::send_alert("⏸ Kingfisher paused via dashboard").await;
            StatusCode::OK
        }
        "resume" => {
            {
                let s = state.read().await;
                if !s.aave_status.reserve_active {
                    tracing::warn!("Cannot resume — Aave reserve not active");
                    return StatusCode::CONFLICT;
                }
            }
            {
                let mut s = state.write().await;
                s.running = true;
                s.consecutive_reverts = 0;
            }
            tracing::info!("Bot RESUMED via dashboard");
            crate::alerts::send_alert("▶️ Kingfisher resumed via dashboard").await;
            StatusCode::OK
        }
        _ => StatusCode::BAD_REQUEST,
    }
}

/// POST /api/withdraw — signal profit withdrawal request
pub async fn trigger_withdrawal(
    State(state): State<SharedState>,
) -> impl IntoResponse {
    let mut s = state.write().await;
    s.pending_withdrawal = true;
    tracing::info!("Withdrawal requested via dashboard");
    StatusCode::OK
}

// ─── WebSocket ───────────────────────────────────────────────────────────────

/// GET /ws — streams full BotState JSON every second.
/// API key is now read from the Sec-WebSocket-Protocol header, NOT a query
/// parameter. Query params appear verbatim in nginx/reverse-proxy access logs and any
/// log aggregation service (Datadog, Papertrail) — storing the key in plaintext
/// in third-party systems. WebSocket Upgrade headers are NOT logged by default.
///
/// The dashboard sends: new WebSocket(url, ["kingfisher-v1", API_KEY])
/// The browser includes both as Sec-WebSocket-Protocol values in the Upgrade request.
/// We verify the second subprotocol value matches the configured API_KEY.
pub async fn ws_handler(
    ws:           WebSocketUpgrade,
    headers:      axum::http::HeaderMap,
    State(state): State<SharedState>,
) -> impl IntoResponse {
    let api_key = std::env::var("API_KEY").unwrap_or_default();
    if api_key.is_empty() {
        return (StatusCode::INTERNAL_SERVER_ERROR, "API_KEY not configured on server").into_response();
    }
    // Sec-WebSocket-Protocol header contains comma-separated subprotocol values.
    // Dashboard sends: "kingfisher-v1, <api_key>"
    let provided_key = headers
        .get("sec-websocket-protocol")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.split(',').nth(1))
        .map(|s| s.trim())
        .unwrap_or("");

    if !crate::auth::constant_time_eq(provided_key, &api_key) {
        return (StatusCode::UNAUTHORIZED, "Invalid or missing API key in Sec-WebSocket-Protocol").into_response();
    }

    ws.on_upgrade(|socket| handle_socket(socket, state)).into_response()
}

async fn handle_socket(socket: WebSocket, state: SharedState) {
    let (mut sender, mut receiver) = socket.split();

    // Send full state immediately on connect
    {
        let s = state.read().await;
        if let Ok(json) = serde_json::to_string(&*s) {
            let _ = sender.send(Message::Text(json)).await;
        }
    }

    let mut tick = interval(Duration::from_secs(1));

    loop {
        tokio::select! {
            _ = tick.tick() => {
                let s = state.read().await;
                match serde_json::to_string(&*s) {
                    Ok(json) => {
                        if sender.send(Message::Text(json)).await.is_err() {
                            break;
                        }
                    }
                    Err(e) => tracing::error!(error = ?e, "State serialization failed"),
                }
            }
            msg = receiver.next() => {
                match msg {
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => {}
                }
            }
        }
    }

    tracing::debug!("WebSocket client disconnected");
}

// ─── Prometheus metrics ────────────────────────────────────────────

/// GET /metrics — Prometheus text format scrape endpoint.
/// Unauthenticated — Prometheus scrapes this; restrict at the network/firewall layer.
pub async fn get_metrics(State(state): State<SharedState>) -> impl IntoResponse {
    // Sync current state into gauges before rendering
    {
        let s = state.read().await;
        crate::metrics::sync_state_metrics(&s);
    }
    let body = crate::metrics::render_metrics();
    (
        StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, "text/plain; version=0.0.4")],
        body,
    )
}
