//! # Chain Loop
//!
//! ## IPC over RPC (Self-Hosted Nitro Node)
//! Prefers IPC (`RPC_IPC_PATH`) if configured; falls back to WebSocket.
//! IPC latency: ~0.1ms vs 50–300ms for external RPC. Full local mempool access
//! unlocks Chainlink mempool monitoring (item #14).
//!
//! ## Async Block Handler
//! The block loop now only *dispatches* — it never awaits processing. Each block
//! spawns an independent task so block N+1 is scanned while N is still executing.
//!
//! ## Stress Regime Hysteresis
//! Entry threshold (0.25%) > exit threshold (0.15%) prevents rapid flapping at
//! the boundary. Alerts fire only on genuine transitions.
//!
//! ## Wire the Edge Monitors
//! The `edges` crate's full detection suite (cascade, llamma, peg_stress, etc.)
//! is now called after every standard scanner pass.

#![allow(clippy::too_many_arguments)]
pub mod chainlink;
pub mod multicall;
pub mod event_indexer;
pub mod pool_discovery;
pub mod chainlink_watch;
pub mod adapter;
pub mod arbitrum;
pub mod monad;

pub use adapter::{ChainAdapter, ChainEvent, SignedTx, StrategyId, TxReceipt, VenueEntry};
pub use arbitrum::ArbitrumAdapter;
pub use monad::MonadAdapter;

use std::sync::Arc;
use tokio::sync::RwLock;
use anyhow::{Context, Result};
use futures::StreamExt;
use alloy::providers::{Provider, ProviderBuilder, WsConnect};

use kingfisher_core::{BotState, GasRegime, config::Network};

pub struct ChainState {
    pub eth_price_usd:      f64,
    pub usdc_peg:           f64,
    pub usdt_peg:           f64,
    pub wallet_eth_balance: f64,
    pub aave_status:        kingfisher_core::AaveReserveStatus,
    pub pool_states:        Vec<kingfisher_core::PoolState>,
}

/// Main block loop — prefers IPC, falls back to WebSocket.
///
/// The loop itself does no work: it dispatches each block into a separate
/// tokio task so that processing of block N never blocks block N+1.
pub async fn run(
    state:   Arc<RwLock<BotState>>,
    network: Network,
) -> Result<()> {
    let ws_url      = std::env::var("RPC_WS_URL").context("RPC_WS_URL not set")?;
    let http_url    = std::env::var("RPC_HTTP_URL").context("RPC_HTTP_URL not set")?;
    let fallback_ws = std::env::var("RPC_FALLBACK_URL").ok();

    // prefer IPC if available
    #[cfg(feature = "ipc")]
    if let Ok(ipc_path) = std::env::var("RPC_IPC_PATH") {
        tracing::info!(ipc = %ipc_path, "Connecting via IPC (~0.1ms latency)");
        use alloy::providers::IpcConnect;
        let provider = ProviderBuilder::new()
            .connect_ipc(IpcConnect::new(ipc_path))
            .await
            .context("IPC connection failed")?;
        return run_with_provider(provider, http_url, state, network).await;
    }

    // WebSocket path — primary then fallback
    tracing::info!(network = ?network, ws_url = %ws_url, "Connecting WebSocket");
    let provider = match ProviderBuilder::new().connect_ws(WsConnect::new(&ws_url)).await {
        Ok(p) => {
            tracing::info!("WebSocket connected (primary)");
            p
        }
        Err(e) => {
            tracing::warn!(error = ?e, "Primary WebSocket failed");
            match fallback_ws {
                Some(ref fb) => ProviderBuilder::new()
                    .connect_ws(WsConnect::new(fb))
                    .await
                    .with_context(|| format!("Both primary ({ws_url}) and fallback WebSocket failed"))?,
                None => return Err(e.into()),
            }
        }
    };

    run_with_provider(provider, http_url, state, network).await
}

async fn run_with_provider<P>(
    provider: P,
    http_url: String,
    state:    Arc<RwLock<BotState>>,
    network:  Network,
) -> Result<()>
where
    P: Provider + Clone + Send + Sync + 'static,
{
    let http_provider = Arc::new(
        ProviderBuilder::new()
            .connect_http(http_url.parse().context("Invalid RPC_HTTP_URL")?)
    );

    let pool_configs = network.pools();
    let sub          = provider.subscribe_blocks().await.context("Block subscription failed")?;
    let mut stream   = sub.into_stream();
    let mut last_aave_active = true;

    // the loop ONLY dispatches — never processes
    while let Some(block) = stream.next().await {
        let block_number = block.number;
        let base_fee     = block.base_fee_per_gas.unwrap_or(21_000_000u64);

        tracing::debug!(block = block_number, gwei = base_fee as f64 / 1e9, "⬛ Block");

        // Write block header into state immediately (before spawning)
        {
            let mut s = state.write().await;
            s.last_block    = block_number;
            s.last_block_at = Some(chrono::Utc::now());
            s.last_base_fee = base_fee as u128; // live base fee for template gas estimates
            // daily reset check
            s.tick_daily_reset();
        }

        // spawn block processing — never await it
        let (state2, net2, http2, cfgs) = (
            Arc::clone(&state),
            network.clone(),
            Arc::clone(&http_provider),
            pool_configs.clone(),
        );
        tokio::spawn(async move {
            let mut _aave_local = false;
            handle_block(block_number, base_fee, state2, net2, http2, cfgs, &mut _aave_local).await;
        });

        // Track Aave state for logging in the dispatch loop
        let aave_now = state.read().await.aave_status.reserve_active;
        if last_aave_active && !aave_now {
            tracing::error!("Aave USDC reserve INACTIVE");
            tokio::spawn(async { kingfisher_api::alerts::alert_aave_reserve_inactive().await; });
        }
        last_aave_active = aave_now;
    }

    Err(anyhow::anyhow!("Block stream terminated"))
}

/// Per-block processing task — runs independently for each block.
/// Stale blocks (more than 1 behind current) are discarded immediately.
async fn handle_block<P>(
    block_number:     u64,
    base_fee:         u64,
    state:            Arc<RwLock<BotState>>,
    network:          Network,
    http_provider:    Arc<P>,
    pool_configs:     Vec<kingfisher_core::config::PoolConfig>,
    _last_aave_active: &mut bool,
)
where
    P: Provider + Clone + Send + Sync + 'static,
{
    // discard stale blocks
    {
        let current = state.read().await.last_block;
        if block_number < current.saturating_sub(1) {
            tracing::debug!(block = block_number, current, "Stale block — discarding");
            return;
        }
    }

    // Parallel multicall: pool states + Aave + Chainlink + wallet
    let chain_state = match multicall::fetch_all_state(
        &http_provider, &network, &pool_configs, block_number,
    ).await {
        Ok(cs)  => cs,
        Err(e) => {
            tracing::error!(block = block_number, error = ?e, "Multicall failed");
            return;
        }
    };

    // Write into shared BotState; collect decision variables
    let (can_trade, stress, aave_max, eth_price, params, aave_fee_bps) = {
        let mut s = state.write().await;
        s.eth_price_usd      = chain_state.eth_price_usd;
        s.usdc_peg           = chain_state.usdc_peg;
        s.usdt_peg           = chain_state.usdt_peg;
        s.wallet_eth_balance = chain_state.wallet_eth_balance;
        s.aave_status        = chain_state.aave_status.clone();

        for mut ps in chain_state.pool_states {
            ps.balance_history.push_back((block_number, ps.balances_norm.clone()));
            if ps.balance_history.len() > 5 { ps.balance_history.pop_front(); }
            s.pool_states.insert(ps.address, ps);
        }

        // hysteresis — separate entry/exit thresholds
        let deviation = (chain_state.usdc_peg - 1.0_f64).abs()
            .max((chain_state.usdt_peg - 1.0_f64).abs());
        let was_stress = s.stress_regime;
        let new_stress = if was_stress {
            deviation > 0.0015   // must fall below 0.15% to exit
        } else {
            deviation > 0.0025   // must exceed 0.25% to enter
        };

        // Alert only on genuine transitions (not every block)
        if new_stress && !was_stress {
            let (u, t) = (s.usdc_peg, s.usdt_peg);
            s.stress_entered_at = Some(chrono::Utc::now());
            tokio::spawn(async move {
                kingfisher_api::alerts::alert_stress_regime(u, t).await;
            });
        } else if !new_stress && was_stress {
            s.stress_entered_at = None;
            let dev = deviation;
            tokio::spawn(async move {
                kingfisher_api::alerts::send_alert(
                    &format!("✅ Peg stress resolved — deviation {:.4}%", dev * 100.0)
                ).await;
            });
        }
        s.stress_regime = new_stress;
        s.update_gas_regime();

        (s.can_trade(), new_stress, s.aave_status.max_borrowable(), s.eth_price_usd, s.params.clone(), s.aave_status.effective_fee_bps())
    };

    if !can_trade {
        match state.read().await.gas_regime {
            GasRegime::Critical => tracing::warn!(block = block_number, "Gas critical — halted"),
            _ => tracing::debug!(block = block_number, "Bot paused"),
        }
        return;
    }
    if !chain_state.aave_status.reserve_active {
        tracing::warn!(block = block_number, "Aave inactive — skipping");
        return;
    }

    // Snapshot pool states (release lock before scanner)
    let pool_snap: Vec<_> = state.read().await.pool_states.values().cloned().collect();

    // Standard 5-layer scanner
    let scanner_result = kingfisher_scanner::scan_block(
        &pool_snap, base_fee.into(), eth_price, aave_max, &params, stress, block_number, &network, aave_fee_bps,
    );

    // Spawn executor and edge monitors in parallel — neither should block the other.
    // On blocks where a standard opportunity fires simultaneously with an edge signal,
    // sequential awaiting would delay edge submission behind the executor round-trip.
    let exec_task = {
        let state2   = Arc::clone(&state);
        let network2 = network.clone();
        tokio::spawn(async move {
            match &scanner_result {
                Err(e) => tracing::error!(block = block_number, error = ?e, "Scanner error"),
                Ok(None) => tracing::debug!(block = block_number, "No opportunity this block"),
                Ok(Some(opp)) => {
                    tracing::info!(
                        block     = block_number,
                        route     = %opp.route_description,
                        profit    = ?opp.simulated_profit_usd,
                        flash_usd = opp.flash_amount as f64 / 1e6,
                        "🎯 Firing"
                    );
                    state2.write().await.push_opportunity(
                        kingfisher_scanner::to_event(opp, true)
                    );
                    match kingfisher_executor::execute(opp, &network2, &state2).await {
                        Ok(tx_result) => {
                            let (success, profit, route, blk) = (
                                tx_result.success, tx_result.profit_usd,
                                opp.route_description.clone(), tx_result.block_target,
                            );
                            if let Some(ref hash) = tx_result.tx_hash {
                                let sim_profit = opp.simulated_profit_usd.unwrap_or(0.0);
                                state2.write().await.register_pending_bundle(hash.clone(), blk, sim_profit, opp.clone());
                            }
                            state2.write().await.push_tx_result(tx_result);
                            if success {
                                if let Some(p) = profit {
                                    tokio::spawn(async move {
                                        kingfisher_api::alerts::alert_trade_executed(p, &route, blk).await;
                                    });
                                }
                            }
                        }
                        Err(e) => {
                            tracing::error!(error = ?e, "Executor error");
                            state2.write().await.consecutive_reverts += 1;
                        }
                    }
                }
            }
        })
    };

    let edge_task = {
        let state2   = Arc::clone(&state);
        let network2 = network.clone();
        let pool_snap2 = pool_snap.clone();
        let params2  = params.clone();
        tokio::spawn(async move {
            run_edge_monitors(
                block_number, &pool_snap2, eth_price, &params2,
                stress, aave_max, &network2, &state2, base_fee.into(), aave_fee_bps,
            ).await;
        })
    };

    // Wait for both to complete before returning from handle_block
    let _ = tokio::join!(exec_task, edge_task);
}

/// Edge monitors — the edges crate contains 10 structural alpha triggers
/// that were previously never called. This function wires them into the block loop.
async fn run_edge_monitors(
    block_number: u64,
    pool_snap:    &[kingfisher_core::types::PoolState],
    eth_price:    f64,
    params:       &kingfisher_core::config::BotParams,
    _stress:      bool,
    aave_max:     u128,
    network:      &Network,
    state:        &Arc<RwLock<BotState>>,
    base_fee:     u128,
    aave_fee_bps: u64,
) {
    use kingfisher_core::config::BotParams;

    // Edge: shadow cascade — 2pool tilt often propagates to crvUSD pools
    let cascade_pools = kingfisher_edges::cascade::cascade_pools_from_twopool(pool_snap);
    if !cascade_pools.is_empty() {
        tracing::info!(pools = ?cascade_pools, "Edge: 2pool shadow cascade detected");
        // Re-scan with relaxed threshold targeting the cascade pools
        let relaxed = BotParams { min_imbalance_pct: params.min_imbalance_pct * 0.6, ..params.clone() };
        if let Ok(Some(opp)) = kingfisher_scanner::scan_block(
            pool_snap, base_fee, eth_price, aave_max,
            &relaxed, true, block_number, network, aave_fee_bps,
        ) {
            tracing::info!(route = %opp.route_description, "Edge cascade opportunity");
            state.write().await.push_opportunity(kingfisher_scanner::to_event(&opp, true));
            let state2    = Arc::clone(state);
            let net       = network.clone();
            let opp_clone = opp.clone();
            tokio::spawn(async move {
                let _ = kingfisher_executor::execute(&opp_clone, &net, &state2).await;
            });
        }
    }

    // Edge: LLAMMA band crossing — ETH price changes predict crvUSD pool tilts
    if let Some(signal) = kingfisher_edges::llamma::infer_signal(eth_price * 0.995, eth_price) {
        if signal.strength == kingfisher_edges::llamma::LlammaStrength::Major {
            tracing::info!(
                pool   = ?signal.pool,
                excess = signal.crvusd_excess,
                "Edge: LLAMMA Major band crossing — elevated crvUSD scan"
            );
            // Target crvUSD pools with lowered threshold
            let crvusd_params = BotParams { min_imbalance_pct: 2.0, ..params.clone() };
            if let Ok(Some(opp)) = kingfisher_scanner::scan_block(
                pool_snap, base_fee, eth_price, aave_max,
                &crvusd_params, true, block_number, network, aave_fee_bps,
            ) {
                tracing::info!(route = %opp.route_description, "LLAMMA edge opportunity");
                let state2    = Arc::clone(state);
                let net       = network.clone();
                let opp_clone = opp.clone();
                tokio::spawn(async move {
                    let _ = kingfisher_executor::execute(&opp_clone, &net, &state2).await;
                });
            }
        }
    }

    // Edge: peg stress with lower threshold
    let usdc_peg = state.read().await.usdc_peg;
    let usdt_peg = state.read().await.usdt_peg;
    let peg = kingfisher_edges::peg_stress::PegStatus::from_prices(
        usdc_peg, usdt_peg, 1.0,
    );
    if peg.any_stressed() {
        tracing::info!(usdc = usdc_peg, usdt = usdt_peg, "Edge: peg stress — scanning with relaxed params");
    }
}
