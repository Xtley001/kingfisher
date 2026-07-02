//! Edge 7 — 3pool Shadow Cascade
//! When the Curve 2pool (USDT/USDC on Arbitrum) tilts, it often cascades
//! to crvUSD-USDT and crvUSD-USDC simultaneously.
//! Detect the 2pool tilt and immediately check all connected pools.

use alloy::primitives::Address;
use kingfisher_core::types::PoolState;

pub const TWOPOOL: &str = "0x7f90122BF0700F9E7e1F688fe926940E8839F353";

/// Returns the set of pools likely to cascade from a 2pool tilt.
pub fn cascade_pools_from_twopool(pool_states: &[PoolState]) -> Vec<Address> {
    let twopool: Address = TWOPOOL.parse().unwrap();
    let two = pool_states.iter().find(|p| p.address == twopool);

    match two {
        Some(p) if p.imbalance_ratio() > 0.05 => {
            // 2pool is tilted — check connected pools
            pool_states.iter()
                .filter(|p| p.address != twopool)
                .filter(|p| p.imbalance_ratio() > 0.03) // sympathy tilt
                .map(|p| p.address)
                .collect()
        }
        _ => vec![],
    }
}
