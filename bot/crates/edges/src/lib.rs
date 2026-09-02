#![allow(clippy::too_many_arguments)]
pub mod cascade;
pub mod gauge_vote;
pub mod liquidation;
pub mod llamma;
pub mod lp_removal;
pub mod peg_stress;
pub mod templates;

use alloy::primitives::Address;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EdgeEvent {
    /// Edge A1: Peg stress — USDC or USDT deviating > 0.2%
    PegStress { token: String, price: f64, deviation_pct: f64 },
    /// Edge A2: LLAMMA band crossing — crvUSD soft liquidation
    LlammaBand { direction: String, band_price: f64 },
    /// Edge A2/A3: 2pool shadow cascade — simultaneous multi-pool tilt
    ThreePoolCascade { pools_affected: Vec<Address> },
    /// Edge A3: Large LP removal — pool tilt predictable
    LpRemoval { pool: Address, amount_usd: f64 },
    /// Edge A4: Gauge vote window (Thursday) — liquidity shifting
    GaugeVoteWindow { epoch_ts: u64 },
    /// Edge A6: Aave V3 lending liquidation bonus widening
    Liquidation { borrower: Address, debt_usd: f64, health_factor: f64 },
}
