#![allow(clippy::too_many_arguments)]
pub mod new_pool;
pub mod convex;
pub mod admin_fee;
pub mod peg_stress;
pub mod gauge_vote;
pub mod cascade;
pub mod bridge;
pub mod lp_removal;
pub mod llamma;
pub mod templates;

use alloy::primitives::Address;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EdgeEvent {
    /// Edge 1: New Curve pool deployed from factory
    NewPool { address: Address, name: String },
    /// Edge 2: Convex harvest window — CRV rewards sold
    ConvexHarvest { pool: Address, amount_crv: u128 },
    /// Edge 3: Admin fee collection — pool fee sweep
    AdminFee { pool: Address },
    /// Edge 4: Peg stress — USDC or USDT deviating > 0.2%
    PegStress { token: String, price: f64, deviation_pct: f64 },
    /// Edge 6: Gauge vote window (Thursday) — liquidity shifting
    GaugeVoteWindow { epoch_ts: u64 },
    /// Edge 7: 3pool shadow cascade — simultaneous multi-pool tilt
    ThreePoolCascade { pools_affected: Vec<Address> },
    /// Edge 8: Cross-bridge arrival — large stablecoin hitting a pool
    BridgeArrival { token: String, amount_usd: f64, eta_blocks: u64 },
    /// Edge 9: Large LP removal — pool tilt predictable
    LpRemoval { pool: Address, amount_usd: f64 },
    /// Edge 10: LLAMMA band crossing — crvUSD soft liquidation
    LlammaBand { direction: String, band_price: f64 },
}
