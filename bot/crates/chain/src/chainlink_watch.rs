//! # Chainlink Mempool Monitoring
//!
//! Watches the local mempool for Chainlink `transmit()` transactions 200–800ms
//! before they are included in a block. A pending price update of > 0.5% triggers
//! pre-computation of arb bundles using the expected post-update price.
//!
//! **Requires a self-hosted Arbitrum Nitro node (item #1).** External RPCs do
//! not expose the full local mempool, only transaction hashes.

use std::sync::Arc;
use alloy::primitives::Address;
use alloy::providers::Provider;
use alloy::consensus::Transaction as _; // brings .to()/.input() into scope on RPC Transaction
use futures::StreamExt;
use tokio::sync::RwLock;

use kingfisher_core::state::BotState;

/// Chainlink `transmit(bytes,bytes32[],bytes32[],bytes32)` selector
const TRANSMIT_SELECTOR: [u8; 4] = [0xc9, 0x80, 0x75, 0x39];

/// Subscribe to full pending transactions from the local node's mempool.
/// Filters for Chainlink ETH/USD feed transactions and pre-builds bundles
/// when a significant price move is pending.
///
/// **Only works with a self-hosted node** — IPC path set via `RPC_IPC_PATH`.
/// Exits cleanly if `subscribe_full_pending_transactions` is not supported.
pub async fn watch_chainlink_mempool<P: Provider + Clone + 'static>(
    provider:     Arc<P>,
    eth_usd_feed: Address,
    state:        Arc<RwLock<BotState>>,
) -> anyhow::Result<()> {
    tracing::info!(feed = %eth_usd_feed, "Chainlink mempool watcher started");

    let mut pending = match provider.subscribe_full_pending_transactions().await {
        Ok(s)  => s.into_stream(),
        Err(e) => {
            tracing::warn!(
                error = %e,
                "subscribe_full_pending_transactions not supported — \
                 self-hosted node (item #1) required for Chainlink mempool monitoring"
            );
            return Ok(());
        }
    };

    while let Some(tx) = pending.next().await {
        // Filter for Chainlink ETH/USD feed
        let Some(to) = tx.to() else { continue };
        if to != eth_usd_feed { continue; }

        let input = tx.input();
        if input.get(..4) != Some(&TRANSMIT_SELECTOR) { continue; }

        // Decode the pending price from transmit() calldata
        match decode_chainlink_transmit(input) {
            Some(pending_price) => {
                let current_price = state.read().await.eth_price_usd;
                if current_price == 0.0 { continue; }

                let change_pct = ((pending_price - current_price) / current_price).abs();

                tracing::info!(
                    pending_price,
                    current_price,
                    change_pct_bps = change_pct * 10_000.0,
                    "⚡ Chainlink ETH/USD update pending in mempool"
                );

                // >0.5% move — pre-build bundles for the expected new price
                if change_pct > 0.005 {
                    let state2 = Arc::clone(&state);
                    tokio::spawn(async move {
                        pre_build_bundles_for_pending_price(pending_price, state2).await;
                    });
                }
            }
            None => continue,
        }
    }

    Ok(())
}

/// Decode the ETH/USD answer from a Chainlink `transmit()` calldata.
/// The answer is encoded in the `_report` bytes at a well-known offset.
fn decode_chainlink_transmit(input: &[u8]) -> Option<f64> {
    // transmit(bytes _report, bytes32[] _rs, bytes32[] _ss, bytes32 _rawVs)
    // _report contains: observationsTimestamp, observers, observations[], juelsPerFeeCoin
    // The observations are i192[] values — first one is the median price in 8 decimals
    if input.len() < 100 { return None; }

    // Skip selector (4) + report offset (32) + report length (32) = 68 bytes
    // Then parse the observations array — this is a simplification;
    // full decoding requires parsing the ABI-encoded bytes
    // Price is in the first 32 bytes of the observations data (i192, 8 decimals)
    let price_slice = input.get(68..100)?;
    let raw = i128::from_be_bytes(price_slice[16..32].try_into().ok()?);
    if raw <= 0 { return None; }

    Some(raw as f64 / 1e8)
}

/// Pre-build arb bundles assuming `pending_price` is the new ETH price.
/// These bundles are ready to fire the instant the Chainlink tx is confirmed.
async fn pre_build_bundles_for_pending_price(
    pending_price: f64,
    state: Arc<RwLock<BotState>>,
) {
    tracing::info!(
        pending_price,
        "Pre-computing arb bundles for pending ETH price"
    );

    // Snapshot current pool states and run scanner with the pending price
    let (pool_states, params, network, aave_max, base_fee, aave_fee_bps) = {
        let s = state.read().await;
        let ps: Vec<_> = s.pool_states.values().cloned().collect();
        (ps, s.params.clone(), s.network.clone(), s.aave_status.max_borrowable(), s.last_base_fee, s.aave_status.effective_fee_bps())
    };

    match kingfisher_scanner::scan_block(
        &pool_states, base_fee, pending_price, aave_max, &params, false,
        state.read().await.last_block, &network, aave_fee_bps,
    ) {
        Ok(Some(opp)) => {
            tracing::info!(
                route  = %opp.route_description,
                profit = opp.estimated_profit_usd,
                "⚡ Chainlink pre-build: opportunity ready — firing immediately"
            );
            crate::execute_and_track(&opp, &network, &state).await;
        }
        Ok(None)   => tracing::debug!("Chainlink pre-build: no profitable route at pending price"),
        Err(e)     => tracing::warn!(error = %e, "Chainlink pre-build: scanner error"),
    }
}
