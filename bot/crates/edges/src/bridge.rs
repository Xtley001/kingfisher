//! Edge 8 — Cross-Bridge Arrival Monitor
//! Large stablecoin bridge arrivals land in specific Curve pools and tilt them.
//! Stargate ~5min settlement. Arbitrum native bridge ~15min.
//! Monitoring bridge contracts lets us pre-position before the tilt lands.

use alloy::primitives::Address;

/// Stargate USDC pool on Arbitrum — major bridge for USDC flows
pub const STARGATE_USDC_POOL: &str = "0x892785f33CdeE22A30AEF750F285E18c18040c3e";

/// Arbitrum bridge inbox — monitors for large native bridge completions
pub const ARB_BRIDGE_INBOX: &str = "0x4Dbd4fc535Ac27206064B68FfCf827b0A60BAB3f";

/// A detected bridge arrival event
#[derive(Debug, Clone)]
pub struct BridgeArrival {
    /// Token being bridged
    pub token:        Address,
    pub token_symbol: String,
    /// Amount in USD
    pub amount_usd:   f64,
    /// Which Curve pool will likely receive it
    pub dest_pool:    Option<Address>,
    /// Estimated blocks until the arrival lands in the pool
    pub eta_blocks:   u64,
}

impl BridgeArrival {
    /// Is this arrival large enough to cause a tradeable imbalance?
    /// Below $100k, the pool absorbs it without meaningful tilt.
    pub fn is_significant(&self) -> bool {
        self.amount_usd > 100_000.0
    }

    /// Expected imbalance magnitude (rough estimate)
    pub fn estimated_imbalance_pct(&self, pool_tvl_usd: f64) -> f64 {
        if pool_tvl_usd <= 0.0 { return 0.0; }
        (self.amount_usd / pool_tvl_usd) * 100.0
    }
}
