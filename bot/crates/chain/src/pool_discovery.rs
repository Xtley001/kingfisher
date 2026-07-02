//! # Curve Pool Auto-Discovery
//!
//! Polls the Curve factory contract every 5 minutes and adds newly deployed
//! stablecoin pools (A parameter > 100) to the known pool list.
//! Fires a Telegram alert for every new pool discovered.

use std::sync::Arc;
use std::time::Duration;
use alloy::primitives::{Address, Bytes, U256};
use alloy::providers::Provider;
use alloy::network::TransactionBuilder;
use alloy::rpc::types::TransactionRequest;
use tokio::sync::RwLock;

use kingfisher_core::config::Network;
use kingfisher_core::state::BotState;

/// Background task: poll Curve factory every 5 minutes; add new stablecoin pools.
pub async fn run_pool_discovery<P: Provider + Clone + 'static>(
    provider: Arc<P>,
    network:  Network,
    state:    Arc<RwLock<BotState>>,
) -> anyhow::Result<()> {
    let Some(factory) = network.curve_factory() else {
        tracing::info!("Pool discovery disabled (no factory configured)");
        return Ok(());
    };

    tracing::info!(factory = %factory, "Pool auto-discovery started (5min interval)");

    loop {
        tokio::time::sleep(Duration::from_secs(300)).await;

        match check_for_new_pools(&provider, factory, &state).await {
            Ok(0)  => tracing::debug!("Pool discovery: no new pools"),
            Ok(n)  => tracing::info!(new_pools = n, "New Curve pools discovered"),
            Err(e) => tracing::warn!(error = %e, "Pool discovery error"),
        }
    }
}

async fn check_for_new_pools<P: Provider + Clone + 'static>(
    provider: &Arc<P>,
    factory:  Address,
    state:    &Arc<RwLock<BotState>>,
) -> anyhow::Result<u64> {
    // pool_count() — selector: keccak256("pool_count()")[0..4]
    let count_sel: [u8; 4] = [0x95, 0x6a, 0xcd, 0xa1];
    let total = call_u256(provider, factory, &count_sel, &[]).await
        .unwrap_or(U256::ZERO);
    let total: u64 = total.try_into().unwrap_or(0);

    let known = state.read().await.pool_states.len() as u64;
    if total <= known { return Ok(0); }

    let mut added = 0u64;

    for i in known..total {
        // pool_list(uint256 index) — selector: keccak256("pool_list(uint256)")[0..4]
        let list_sel: [u8; 4] = [0xb1, 0x54, 0x81, 0x75];
        let mut idx_bytes = [0u8; 32];
        idx_bytes[24..].copy_from_slice(&i.to_be_bytes());
        let addr_raw = match call_bytes32(provider, factory, &list_sel, &idx_bytes).await {
            Some(b) => b,
            None    => continue,
        };
        let addr = Address::from_slice(&addr_raw[12..32]);

        // A() — selector: keccak256("A()")[0..4]
        let a_sel: [u8; 4] = [0xf4, 0x46, 0xc1, 0xd0];
        let a_val = call_u256(provider, addr, &a_sel, &[]).await.unwrap_or(U256::ZERO);
        let a_param: u64 = a_val.try_into().unwrap_or(0);

        // Only stablecoin pools (A > 100)
        if a_param <= 100 { continue; }

        tracing::info!(pool = %addr, a_param, index = i, "🆕 New Curve stablecoin pool");

        let msg = format!("🆕 New Curve pool: {addr:?} (A={a_param}, index={i})");
        tokio::spawn(async move {
            kingfisher_api::alerts::send_alert(&msg).await;
        });

        added += 1;
    }

    Ok(added)
}

/// Call a view function and decode the first 32 bytes as U256.
async fn call_u256<P: Provider>(
    provider:  &Arc<P>,
    to:        Address,
    selector:  &[u8; 4],
    args:      &[u8],
) -> Option<U256> {
    let mut data = selector.to_vec();
    data.extend_from_slice(args);

    let tx = TransactionRequest::default()
        .to(to)
        .with_input(Bytes::from(data));

    let result = provider.call(tx).await.ok()?;
    if result.len() < 32 { return None; }
    Some(U256::from_be_slice(&result[result.len()-32..]))
}

/// Call a view function and return the raw 32-byte response.
async fn call_bytes32<P: Provider>(
    provider:  &Arc<P>,
    to:        Address,
    selector:  &[u8; 4],
    args:      &[u8],
) -> Option<Vec<u8>> {
    let mut data = selector.to_vec();
    data.extend_from_slice(args);

    let tx = TransactionRequest::default()
        .to(to)
        .with_input(Bytes::from(data));

    let result = provider.call(tx).await.ok()?;
    if result.len() < 32 { return None; }
    Some(result[result.len()-32..].to_vec())
}
