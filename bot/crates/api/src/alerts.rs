use reqwest::Client;

static CLIENT: std::sync::OnceLock<Client> = std::sync::OnceLock::new();

fn http_client() -> &'static Client {
    CLIENT.get_or_init(Client::new)
}

/// Fire-and-forget Telegram alert. Never blocks the bot.
pub async fn send_alert(msg: &str) {
    let token   = match std::env::var("TELEGRAM_BOT_TOKEN") { Ok(t) => t, Err(_) => return };
    let chat_id = match std::env::var("TELEGRAM_CHAT_ID")   { Ok(t) => t, Err(_) => return };

    let url  = format!("https://api.telegram.org/bot{}/sendMessage", token);
    let text = format!("🦅 *Kingfisher*\n{}", msg);

    let body = serde_json::json!({
        "chat_id":    chat_id,
        "text":       text,
        "parse_mode": "Markdown"
    });

    if let Err(e) = http_client().post(&url).json(&body).send().await {
        tracing::warn!(error = ?e, "Telegram alert failed (non-critical)");
    }
}

// ─── Named alert functions ────────────────────────────────────────────────────

pub async fn alert_trade_executed(profit_usd: f64, route: &str, block: u64) {
    send_alert(&format!(
        "✅ Trade landed\nProfit: ${:.2}\nRoute: {}\nBlock: #{}",
        profit_usd, route, block
    )).await;
}

pub async fn alert_aave_reserve_inactive() {
    send_alert("🚨 CRITICAL: Aave USDC reserve frozen or paused\nBot auto-halted").await;
}

pub async fn alert_consecutive_reverts(count: u32) {
    send_alert(&format!(
        "⚠️ {} consecutive tx failures\nBot auto-paused — check logs", count
    )).await;
}

pub async fn alert_gas_critical(balance_eth: f64, floor_eth: f64) {
    send_alert(&format!(
        "⛽ Gas CRITICAL\nBalance: {:.4} ETH\nFloor: {:.4} ETH\nBot halted — refill bot wallet",
        balance_eth, floor_eth
    )).await;
}

pub async fn alert_gas_low(balance_eth: f64) {
    send_alert(&format!(
        "⚠️ Gas low: {:.4} ETH\nRefill bot wallet soon (target: 1.0 ETH)",
        balance_eth
    )).await;
}

pub async fn alert_stress_regime(usdc: f64, usdt: f64) {
    send_alert(&format!(
        "⚡ PEG STRESS DETECTED\nUSDC: ${:.4} | USDT: ${:.4}\nOptimal sizing mode active",
        usdc, usdt
    )).await;
}

pub async fn alert_new_pool(pool_addr: &str) {
    send_alert(&format!(
        "🆕 New Curve pool detected\n{}\n24h monitoring before trading",
        pool_addr
    )).await;
}

pub async fn alert_sim_divergence(divergence_pct: f64) {
    send_alert(&format!(
        "⚠️ Simulation divergence: {:.2}%\nBot auto-paused — algebraic vs eth_call mismatch",
        divergence_pct
    )).await;
}

// ─── §15.2 Required monitoring alerts ─────────────────────────────────────────

pub async fn alert_low_landing_rate(rate_pct: f64, window_mins: u64) {
    send_alert(&format!(
        "🚨 Landing rate critically low: {:.1}% over {}min window\nInclusion failing — check sequencer endpoint, RPC health, and gas tip",
        rate_pct, window_mins
    )).await;
}

pub async fn alert_gas_spent_no_profit(gas_usd: f64, window_mins: u64) {
    send_alert(&format!(
        "🚨 Gas drain alert: ${:.2} spent in {}min with ZERO confirmed profit\nBot auto-paused — investigate immediately",
        gas_usd, window_mins
    )).await;
}

pub async fn alert_no_blocks_processed(seconds_since: u64) {
    send_alert(&format!(
        "🚨 NODE OUTAGE: No blocks processed for {}s\nCheck RPC endpoint health — bot is blind",
        seconds_since
    )).await;
}
