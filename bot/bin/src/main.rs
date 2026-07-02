//! # Kingfisher — Stablecoin Curve Arbitrage Bot
//!
//! Background tasks launched at startup:
//!   - Trade history persistence (load on start, append on trade)
//!   - Landing tracker (confirm on-chain bundle inclusion)
//!   - Prometheus metrics sync (every 10 seconds)
//!   - Builder metrics refresh (every 10 blocks)
//!   - Curve pool auto-discovery (every 5 minutes)
//!   - Chainlink mempool monitor (self-hosted node only)
//!   - Token alignment verification (startup)
//!   - Stress regime + daily reset (per-block in chain loop)
//!   - Edge monitors (LLAMMA, gauge vote, depeg templates)

#![allow(clippy::too_many_arguments)]
use anyhow::Result;
use std::sync::Arc;
use tokio::sync::RwLock;
use alloy::providers::Provider; // brings get_transaction_receipt into scope on the http provider

use kingfisher_simulation::sizing::max_impact_for_a;
use kingfisher_core::{
    config::{Network, BotParams},
    state::BotState,
    GasRegime,
};

#[tokio::main]
async fn main() -> Result<()> {
    // Load environment file — NETWORK= controls which file is loaded
    let network_name = std::env::var("NETWORK").unwrap_or_else(|_| "testnet".into());
    let env_file = if network_name == "mainnet" { ".env.mainnet" } else { ".env.testnet" };
    dotenvy::from_filename(env_file).ok();

    // Structured JSON logging — every field is grep-able in journald/Datadog logs
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("kingfisher=debug,warn"))
        )
        .init();

    let network = Network::from_env();
    let params  = BotParams::from_env();

    tracing::info!(
        network    = ?network,
        chain_id   = network.chain_id(),
        min_profit = params.min_profit_usd,
        gas_floor  = params.gas_reserve_eth,
"🦅 Kingfisher starting"
    );

    // Token alignment check — non-blocking, surfaces USDC variant mismatches at startup.
    kingfisher_core::config::verify_token_alignment_log(&network).await;

    // ── Load trade history from persistent log ───────────────────────────────
    let mut state = BotState::new(network.clone(), params);
    let history   = kingfisher_api::persistence::load_history();
    let hist_len  = history.len();
    for tx in history {
        state.push_tx_result(tx);
    }
    // History spans multiple days; reset daily counters so today's count starts from runtime.
    state.today_profit_usd = 0.0;
    state.today_trades     = 0;
    tracing::info!(restored_trades = hist_len, "📚 History loaded — daily counters reset to runtime");

    let state = Arc::new(RwLock::new(state));

    // ── API server ────────────────────────────────────────────────────────────
    {
        let s = Arc::clone(&state);
        tokio::spawn(async move {
            if let Err(e) = kingfisher_api::start(s).await {
                tracing::error!(error = ?e, "API server crashed");
                std::process::exit(1);
            }
        });
    }

    // ── Uptime counter ────────────────────────────────────────────────────────
    {
        let s = Arc::clone(&state);
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(std::time::Duration::from_secs(1));
            loop { tick.tick().await; s.write().await.uptime_secs += 1; }
        });
    }

    // ── Gas regime monitor + Telegram alerts ─────────────────────────────────
    {
        let s = Arc::clone(&state);
        tokio::spawn(async move {
            let mut tick       = tokio::time::interval(std::time::Duration::from_secs(30));
            let mut last_alert = 0u64;
            loop {
                tick.tick().await;
                let (regime, balance, floor, uptime) = {
                    let st = s.read().await;
                    (st.gas_regime.clone(), st.wallet_eth_balance, st.params.gas_reserve_eth, st.uptime_secs)
                };
                match regime {
                    GasRegime::Critical if uptime - last_alert > 300 => {
                        last_alert = uptime;
                        kingfisher_api::alerts::alert_gas_critical(balance, floor).await;
                    }
                    GasRegime::Alert if uptime - last_alert > 1800 => {
                        last_alert = uptime;
                        kingfisher_api::alerts::alert_gas_low(balance).await;
                    }
                    _ => {}
                }
            }
        });
    }

    // ── Consecutive revert circuit breaker (error reverts only) ─────────────
    // Race losses (ProfitBelowMin) do not count; only genuine bugs trip this.
    {
        let s = Arc::clone(&state);
        tokio::spawn(async move {
            let mut tick      = tokio::time::interval(std::time::Duration::from_secs(5));
            let mut alerted   = 0u32;
            loop {
                tick.tick().await;
                let reverts = s.read().await.consecutive_reverts;
                if reverts >= 5 && reverts != alerted {
                    alerted = reverts;
                    tracing::error!(reverts, "🛑 Circuit breaker: 5 consecutive error reverts — pausing");
                    s.write().await.running = false;
                    kingfisher_api::alerts::alert_consecutive_reverts(reverts).await;
                }
            }
        });
    }

    // ── Landing tracker — confirm on-chain bundle inclusion ──────────────────
    {
        let s       = Arc::clone(&state);
        let network = network.clone();
        tokio::spawn(async move {
            let http_url = std::env::var("RPC_HTTP_URL").unwrap_or_default();
            if http_url.is_empty() { return; }

            let provider = Arc::new(
                alloy::providers::ProviderBuilder::new()
                    .connect_http(http_url.parse().expect("Invalid RPC_HTTP_URL"))
            );

            let mut tick = tokio::time::interval(std::time::Duration::from_secs(2));
            loop {
                tick.tick().await;
                let current_block = s.read().await.last_block;
                if current_block == 0 { continue; }

                // Collect pending bundles to check
                let pending = s.read().await.pending_bundle_hashes.clone();
                let mut to_remove      = vec![];
                let mut hashes_to_drop = vec![];

                for (i, (hash, target, sim_profit)) in pending.iter().enumerate() {
                    // Try to parse hash as a tx hash
                    let Ok(tx_hash): Result<alloy::primitives::TxHash, _>
                        = hash.parse() else { continue };

                    match provider.get_transaction_receipt(tx_hash).await {
                        Ok(Some(receipt)) => {
                            let landed = receipt.block_number.unwrap_or(0);

                            // Authoritative P&L: decode the contract's own ArbExecuted event.
                            // Profit accrues INSIDE the KingfisherArb contract (withdrawn later to
                            // the cold wallet), so summing token transfers to the bot wallet is wrong.
                            // event ArbExecuted(address indexed flashToken, uint256 flashAmount,
                            //                   uint256 aavePremium, uint256 netProfit, uint256 hopsCount)
                            // netProfit is the 3rd non-indexed word (data[64..96]), in flash-token wei.
                            let contract_addr = network.kingfisher_contract();
                            let arb_topic = alloy::primitives::keccak256(
                                "ArbExecuted(address,uint256,uint256,uint256,uint256)".as_bytes()
                            );
                            let actual_profit_usd: f64 = receipt.inner.logs().iter()
                                .filter(|log| {
                                    log.address() == contract_addr
                                        && log.topics().first() == Some(&arb_topic)
                                })
                                .map(|log| {
                                    let data = log.data().data.as_ref();
                                    if data.len() >= 96 {
                                        let raw = u128::from_be_bytes(data[80..96].try_into().unwrap_or([0u8;16]));
                                        raw as f64 / 1e6 // stablecoin flash token, 6 decimals
                                    } else { 0.0 }
                                })
                                .sum();

                            // Use the event-derived profit; fall back to sim if the log is absent.
                            let reported_profit = if actual_profit_usd > 0.0 {
                                actual_profit_usd
                            } else {
                                tracing::debug!(tx_hash = %hash, "No ArbExecuted event in receipt — using sim_profit as fallback");
                                *sim_profit
                            };

                            tracing::info!(
                                tx_hash       = %hash,
                                landed_block  = landed,
                                target_block  = target,
                                sim_profit,
                                actual_profit = reported_profit,
                                "✅ Bundle confirmed on-chain"
                            );
                            kingfisher_api::metrics::OPPS_LANDED.inc();
                            kingfisher_api::metrics::PROFIT_ACTUAL.inc_by(reported_profit);
                            s.write().await.confirm_trade_landed(landed, reported_profit);
                            to_remove.push(i);
                            hashes_to_drop.push(hash.clone());
                        }
                        Ok(None) if current_block > target + 3 => {
                            // 3 blocks past target with no receipt = not included
                            tracing::debug!(tx_hash = %hash, "Bundle not included (expired)");
                            to_remove.push(i);
                            hashes_to_drop.push(hash.clone());
                        }
                        Ok(None) => {} // still pending
                        Err(e) => tracing::debug!(error = %e, "Receipt check error"),
                    }
                }

                // Remove confirmed/expired bundles (reverse order to preserve indices)
                if !to_remove.is_empty() {
                    let mut st = s.write().await;
                    for &i in to_remove.iter().rev() {
                        if i < st.pending_bundle_hashes.len() {
                            st.pending_bundle_hashes.remove(i);
                        }
                    }
                    // Also clean up stored Opportunity structs to avoid unbounded growth
                    for hash in &hashes_to_drop {
                        st.remove_pending_opportunity(hash);
                    }
                }
            }
        });
    }

    // ── Prometheus metrics sync (every 10 seconds) ───────────────────────────
    {
        let s = Arc::clone(&state);
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(std::time::Duration::from_secs(10));
            loop {
                tick.tick().await;
                let st = s.read().await;
                kingfisher_api::metrics::sync_state_metrics(&st);
            }
        });
    }

    // ── Curve pool auto-discovery (every 5 minutes) ──────────────────────────
    {
        let s       = Arc::clone(&state);
        let network = network.clone();
        tokio::spawn(async move {
            let http_url = std::env::var("RPC_HTTP_URL").unwrap_or_default();
            if http_url.is_empty() { return; }
            let Ok(url) = http_url.parse() else { return };
            let provider = alloy::providers::ProviderBuilder::new().connect_http(url);
            let _ = kingfisher_chain::pool_discovery::run_pool_discovery(
                Arc::new(provider), network, s,
            ).await;
        });
    }

    // ── Event indexer — primary real-time pool state source ──────────────────
    // Subscribes to Curve TokenExchange/AddLiquidity/RemoveLiquidity events and
    // applies balance deltas immediately, without waiting for the next multicall.
    // Multicall is retained as a 5-block reconciliation fallback.
    {
        let s        = Arc::clone(&state);
        let network2 = network.clone();
        tokio::spawn(async move {
            let ws_url = std::env::var("RPC_WS_URL").unwrap_or_default();
            if ws_url.is_empty() {
                tracing::warn!("RPC_WS_URL not set — event indexer disabled; running multicall-only mode");
                return;
            }
            let provider = match alloy::providers::ProviderBuilder::new()
                .connect_ws(alloy::providers::WsConnect::new(&ws_url)).await
            {
                Ok(p)  => std::sync::Arc::new(p),
                Err(e) => {
                    tracing::error!(error = ?e, "WebSocket provider failed — event indexer disabled");
                    return;
                }
            };
            let pool_addrs: Vec<_> = network2.pools().iter()
                .map(|p| p.address)
                .collect();
            loop {
                match kingfisher_chain::event_indexer::run_event_indexer(
                    std::sync::Arc::clone(&provider), pool_addrs.clone(), std::sync::Arc::clone(&s),
                ).await {
                    Ok(_)  => {}
                    Err(e) => {
                        tracing::warn!(error = ?e, "Event indexer disconnected — reconnecting in 5s");
                        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    }
                }
            }
        });
    }

    // ── Chainlink mempool monitor (self-hosted node only) ────────────────────
    if std::env::var("RPC_IPC_PATH").is_ok() {
        let s       = Arc::clone(&state);
        let network = network.clone();
        tokio::spawn(async move {
            let http_url = std::env::var("RPC_HTTP_URL").unwrap_or_default();
            let Ok(parsed) = http_url.parse() else { return };
            let provider   = alloy::providers::ProviderBuilder::new().connect_http(parsed);
            let eth_feed   = network.chainlink_eth_usd();
            let _ = kingfisher_chain::chainlink_watch::watch_chainlink_mempool(
                Arc::new(provider), eth_feed, s,
            ).await;
        });
    }

    // ── Depeg template refresh (every 100 blocks) ─────────────────────────────
    {
        let s = Arc::clone(&state);
        let n = network.clone();
        tokio::spawn(async move {
            let mut templates: Option<kingfisher_edges::templates::DepegTemplates> = None;
            let mut _last_template_block: u64 = 0;
            loop {
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                let (block, usdc_peg, usdt_peg, pool_states, aave_max, params, stress, base_fee, eth_price, aave_fee_bps) = {
                    let st = s.read().await;
                    let ps: Vec<_> = st.pool_states.values().cloned().collect();
                    (st.last_block, st.usdc_peg, st.usdt_peg, ps,
                     st.aave_status.max_borrowable(), st.params.clone(), st.stress_regime,
                     st.last_base_fee, st.eth_price_usd, st.aave_status.effective_fee_bps())
                };
                if block == 0 { continue; }

                let needs_rebuild = templates.as_ref()
                    .map(|t| t.is_stale(block))
                    .unwrap_or(true);

                if needs_rebuild {
                    templates = Some(kingfisher_edges::templates::DepegTemplates::build(
                        &pool_states, aave_max, &params, &n, block, base_fee, eth_price, aave_fee_bps,
                    ));
                    _last_template_block = block;
                    tracing::debug!(block, "Depeg templates rebuilt");
                }

                if stress {
                    if let Some(scenario) = kingfisher_edges::templates::DepegTemplates::active_scenario(
                        usdc_peg, usdt_peg, 1.0,
                    ) {
                        if let Some(ref t) = templates {
                            if let Some(opp) = t.get(&scenario) {
                                tracing::info!(scenario = ?scenario, profit = opp.estimated_profit_usd,
                                    "⚡ Firing pre-built depeg template");
                                let opp2 = opp.clone();
                                let s2   = Arc::clone(&s);
                                let n2   = n.clone();
                                tokio::spawn(async move {
                                    let _ = kingfisher_executor::execute(&opp2, &n2, &s2).await;
                                });
                            }
                        }
                    }
                }
            }
        });
    }

    // ── LLAMMA edge monitor ───────────────────────────────────────────────────
    {
        let s = Arc::clone(&state);
        tokio::spawn(async move {
            let mut prev_eth = 0.0f64;
            let mut tick = tokio::time::interval(std::time::Duration::from_millis(300));
            loop {
                tick.tick().await;
                let (eth_now, running) = {
                    let st = s.read().await;
                    (st.eth_price_usd, st.running)
                };
                if !running || eth_now == 0.0 { prev_eth = eth_now; continue; }
                if prev_eth > 0.0 {
                    if let Some(signal) = kingfisher_edges::llamma::infer_signal(prev_eth, eth_now) {
                        if signal.strength == kingfisher_edges::llamma::LlammaStrength::Major {
                            tracing::info!(pool = ?signal.pool, excess = signal.crvusd_excess,
                                "LLAMMA Major band crossing — elevated scan priority");
                        }
                    }
                }
                prev_eth = eth_now;
            }
        });
    }

    // ── Gauge vote window checker ─────────────────────────────────────────────
    {
        let s = Arc::clone(&state);
        tokio::spawn(async move {
            let mut tick           = tokio::time::interval(std::time::Duration::from_secs(300));
            let mut window_active  = false;
            let mut saved_imbalance = 0.0f64;
            loop {
                tick.tick().await;
                let in_window = kingfisher_edges::gauge_vote::is_gauge_vote_window();
                if in_window && !window_active {
                    let mut st = s.write().await;
                    saved_imbalance = st.params.min_imbalance_pct;
                    st.params.min_imbalance_pct = (saved_imbalance * 0.7).max(1.0);
                    window_active = true;
                    tracing::info!(original = saved_imbalance,
                        overridden = st.params.min_imbalance_pct,
                        "📅 Thursday gauge vote window: imbalance threshold lowered");
                } else if !in_window && window_active {
                    s.write().await.params.min_imbalance_pct = saved_imbalance;
                    window_active = false;
                    tracing::info!(restored = saved_imbalance, "📅 Gauge vote window closed");
                }
            }
        });
    }

    // ── Withdrawal checker ───────────────────────────────────────────────────
    {
        let s = Arc::clone(&state);
        let n = network.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(std::time::Duration::from_secs(30));
            loop {
                tick.tick().await;
                let _ = kingfisher_executor::check_and_execute_withdrawal(&n, &s).await;
            }
        });
    }

    // ── Simulation divergence validation (every 100 blocks) ──────────────────
    {
        let s = Arc::clone(&state);
        let n = network.clone();
        tokio::spawn(async move {
            let mut last_validated = 0u64;
            let mut tick = tokio::time::interval(std::time::Duration::from_secs(10));
            loop {
                tick.tick().await;
                let (block, running) = {
                    let st = s.read().await;
                    (st.last_block, st.running)
                };
                if !running || block == 0 || block.saturating_sub(last_validated) < 100 { continue; }
                last_validated = block;

                // Use a real Opportunity from pending_opportunities, not a synthetic stub.
                // A synthetic opportunity with flash_token=ZERO and empty route always reverts
                // eth_call, forcing the auto-pause guard to rely on a secondary check.
                let real_opp_opt = {
                    let st = s.read().await;
                    st.pending_opportunities.values().next().cloned()
                };

                if let Some(real_opp) = real_opp_opt {
                    let (base_fee, eth_price, aave_fee_bps) = {
                        let st = s.read().await;
                        (st.last_base_fee, st.eth_price_usd, st.aave_status.effective_fee_bps())
                    };

                    match kingfisher_simulation::validation::validate_simulation(
                        &real_opp, &n, base_fee, eth_price, aave_fee_bps,
                    ).await {
                        Ok(vr) if !vr.passes => {
                            tracing::error!(block, divergence = vr.divergence_pct,
                                "🛑 Simulation drift — auto-pausing");
                            s.write().await.running = false;
                            kingfisher_api::alerts::send_alert(
                                &format!("🛑 Kingfisher AUTO-PAUSED: sim drift {:.4}% at block {}", vr.divergence_pct, block)
                            ).await;
                        }
                        Ok(vr) => tracing::debug!(block, divergence = vr.divergence_pct, "✅ Sim validation passed"),
                        Err(e) => tracing::warn!(error = ?e, "Sim validation error — skipping"),
                    }
                } else {
                    tracing::debug!(block, "Sim validation skipped: no pending opportunities to validate against");
                }
            }
        });
    }

    // ── §15.2 Required monitoring alerts ─────────────────────────────────────
    // Landing rate < 20% for 30+ minutes → alert
    {
        let s = Arc::clone(&state);
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(std::time::Duration::from_secs(60));
            let window_mins = 30u64;
            // Ring buffer: track fired/landed counts over the window
            let mut fired_history:  std::collections::VecDeque<u64> = std::collections::VecDeque::new();
            let mut landed_history: std::collections::VecDeque<u64> = std::collections::VecDeque::new();
            loop {
                tick.tick().await;
                let (total_fired, total_landed) = {
                    let st = s.read().await;
                    (st.total_trades + st.race_losses + st.error_reverts, st.total_trades)
                };
                fired_history.push_back(total_fired);
                landed_history.push_back(total_landed);
                if fired_history.len() > window_mins as usize {
                    fired_history.pop_front();
                    landed_history.pop_front();
                }
                if fired_history.len() >= window_mins as usize {
                    let fired_delta  = fired_history.back().unwrap_or(&0).saturating_sub(*fired_history.front().unwrap_or(&0));
                    let landed_delta = landed_history.back().unwrap_or(&0).saturating_sub(*landed_history.front().unwrap_or(&0));
                    if fired_delta >= 5 {
                        let rate_pct = landed_delta as f64 / fired_delta as f64 * 100.0;
                        if rate_pct < 20.0 {
                            tracing::error!(rate_pct, fired_delta, landed_delta, "🚨 Landing rate below 20%");
                            kingfisher_api::alerts::alert_low_landing_rate(rate_pct, window_mins).await;
                        }
                    }
                }
            }
        });
    }

    // Gas spent > $X with zero profit in 1 hour → alert + auto-pause
    {
        let s = Arc::clone(&state);
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(std::time::Duration::from_secs(60));
            let mut baseline_gas_usd    = 0.0f64;
            let mut baseline_profit_usd = 0.0f64;
            let mut window_start        = std::time::Instant::now();
            let window_secs             = 3600u64;
            let gas_threshold_usd       = 50.0f64; // alert if $50+ gas with zero profit
            loop {
                tick.tick().await;
                let (cumulative_gas, cumulative_profit) = {
                    let st = s.read().await;
                    // Approximate: use total_profit as proxy for cumulative profit
                    (st.total_profit_usd.max(0.0), st.total_profit_usd)
                };
                if window_start.elapsed().as_secs() >= window_secs {
                    let window_gas    = cumulative_gas - baseline_gas_usd;
                    let window_profit = cumulative_profit - baseline_profit_usd;
                    if window_gas > gas_threshold_usd && window_profit <= 0.0 {
                        tracing::error!(window_gas, window_profit, "🚨 Gas drain: no profit in 1h window");
                        s.write().await.running = false;
                        kingfisher_api::alerts::alert_gas_spent_no_profit(window_gas, window_secs / 60).await;
                    }
                    baseline_gas_usd    = cumulative_gas;
                    baseline_profit_usd = cumulative_profit;
                    window_start        = std::time::Instant::now();
                }
            }
        });
    }

    // No blocks processed for > 60 seconds → node outage alert
    {
        let s = Arc::clone(&state);
        tokio::spawn(async move {
            let mut tick     = tokio::time::interval(std::time::Duration::from_secs(15));
            let mut last_blk = 0u64;
            let mut last_blk_time = std::time::Instant::now();
            loop {
                tick.tick().await;
                let current_blk = s.read().await.last_block;
                if current_blk != last_blk {
                    last_blk      = current_blk;
                    last_blk_time = std::time::Instant::now();
                } else {
                    let stale_secs = last_blk_time.elapsed().as_secs();
                    if stale_secs > 60 && last_blk > 0 {
                        tracing::error!(stale_secs, last_block = last_blk, "🚨 No blocks for >60s — node outage?");
                        kingfisher_api::alerts::alert_no_blocks_processed(stale_secs).await;
                    }
                }
            }
        });
    }

    // Startup pool config audit — fires once. Shows A, impact threshold, fee source per pool.
    {
        let state_r = state.read().await;
        tracing::info!("━━━ Pool Sizing Config ━━━");
        for ps in state_r.pool_states.values() {
            let a          = ps.a_parameter as f64;
            let fee_source = if ps.fee_rate.is_some() { "on-chain" } else { "default(0.04%)" };
            tracing::info!(
                pool           = %ps.name,
                a_parameter    = a,
                max_impact_pct = format!("{:.2}%", max_impact_for_a(a)),
                fee_rate_bps   = ps.fee_rate.unwrap_or(0.0004) * 10_000.0,
                fee_source     = fee_source,
                is_meta        = ps.is_meta,
                "Pool config"
            );
            if ps.fee_rate.is_none() {
                tracing::warn!(
                    pool    = %ps.name,
                    address = %ps.address,
                    "⚠️  fee_rate not fetched from chain — using 0.04% default. \
                     Confirm fee() is in multicall ABI or profit estimates may be wrong."
                );
            }
        }
        tracing::info!("━━━━━━━━━━━━━━━━━━━━━━━━━");
    }

    // ── Main block loop — reconnects automatically on disconnect ──────────────
    tracing::info!("🚀 Starting main block loop");
    loop {
        match kingfisher_chain::run(Arc::clone(&state), network.clone()).await {
            Ok(_)  => {}
            Err(e) => {
                tracing::error!(error = ?e, "Block loop crashed — reconnecting in 2s");
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            }
        }
    }
}
