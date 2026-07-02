//! Edge 1 — New Pool Launch Monitor
//! Watches Curve Factory for PlainPoolDeployed events.
//! New pools get 48-72h monopoly arb before other bots discover them.

use alloy::primitives::{Address, B256};
use alloy::providers::Provider;
use alloy::rpc::types::Filter;
use anyhow::Result;
use futures::StreamExt;

pub const CURVE_FACTORY: &str = "0xb17b674D9c5CB2e441F8e196a2f048A81355d031";

/// Watch Curve Factory for new pool deployments.
/// `on_new_pool(addr, is_meta)` is called for each new pool.
/// New pools enter a 24h monitoring window before being eligible for trading.
pub async fn watch<P, F>(provider: P, on_new_pool: F) -> Result<()>
where
    P: Provider,
    F: Fn(Address, bool) + Send + 'static,
{
    let factory: Address = CURVE_FACTORY.parse()?;
    let filter = Filter::new().address(factory);
    let mut stream = provider.subscribe_logs(&filter).await?.into_stream();
    while let Some(log) = stream.next().await {
        let topic0 = log.topic0();
        // This is the keccak256 topic hash for the Curve Factory PlainPoolDeployed event.
        // If topic0 matches this hash, it is a plain (non-meta) pool deployment.
        // If topic0 differs, the event is a MetaPoolDeployed — is_meta = true.
        let plain_pool_deployed: B256 = "0xe6e1b7d91f16a5d2c07792e4fd6eef55c7da35ef01b44b5a7a8c5e22fc9e68fa"
            .parse().unwrap_or_default();
        let is_meta = topic0
            .map(|t| t != &plain_pool_deployed)
            .unwrap_or(false);
        if log.data().data.len() >= 32 {
            let mut arr = [0u8; 20];
            arr.copy_from_slice(&log.data().data[12..32]);
            let pool_addr = Address::from(arr);
            if pool_addr != Address::ZERO {
                tracing::info!(pool = ?pool_addr, is_meta, "🆕 New Curve pool — 24h monitoring");
                on_new_pool(pool_addr, is_meta);
            }
        }
    }
    Ok(())
}
