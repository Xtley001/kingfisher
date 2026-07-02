//! Edge 2 — Convex Harvest Windows
//! Convex periodically harvests CRV rewards and sells them for USDC/USDT.
//! This sale systematically pushes certain pools. The harvest schedule is
//! semi-predictable via on-chain Convex contract state.

use alloy::primitives::Address;

/// Convex Booster on Arbitrum
pub const CONVEX_BOOSTER: &str = "0xF403C135812408BFbE8713b5A23a04b3D48AAE31";

#[derive(Debug, Clone)]
pub struct HarvestWindow {
    pub pool:      Address,
    pub crv_amount: u128,
    pub est_usdc_out: f64,
}
