//! # Simulation
//!
//! Opportunity evaluation in two stages:
//!
//! 1. **Algebraic fast path** (`simulate_opportunity`) — float StableSwap math with
//!    dynamic gas estimation. Runs in microseconds, no RPC calls. This is the filter
//!    used by the scanner on every block.
//!
//! 2. **`eth_call` validation** (`validation.rs`) — periodically re-prices a real
//!    pending opportunity against the live contract via `eth_call` and auto-pauses the
//!    bot if the algebraic result diverges by more than the configured threshold.
//!
//! The on-chain `minProfit` guard in `KingfisherArb.executeArb()` is the ultimate
//! source of truth: any trade that would not clear the profit floor reverts, so a
//! simulation error costs at most gas, never principal. A full local EVM re-execution
//! (revm) preflight is a possible future optimization to cut wasted gas on losing
//! races — see docs/STRATEGY.md.

#![allow(clippy::too_many_arguments)]
pub mod sizing;
pub mod spread;
pub mod validation;

use anyhow::Result;
use rayon::prelude::*;
use kingfisher_core::{config::Network, types::Opportunity};

// ── Simulation result ─────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct SimulationResult {
    pub success:          bool,
    pub gross_profit_usd: f64,
    pub aave_fee_usd:     f64,
    pub gas_cost_usd:     f64,
    pub net_profit_usd:   f64,
    pub gas_used:         u64,
    pub revert_reason:    Option<String>,
    pub borrow_amount:    u128,
    /// Reserved for a future EVM-measured gas path; currently always false
    /// (gas is an algebraic estimate cross-checked by the eth_call validator).
    pub gas_is_measured:  bool,
}

// ── Algebraic simulation (fast path) ─────────────────────────────────────────

/// Re-derive net profit using the current base fee and per-hop gas model.
/// Derives from `gross_swap_profit_usd` (raw swap output before fees) to avoid
/// double-subtracting gas that was already deducted at the scanner layer.
///
/// `aave_fee_bps` must be read at runtime from `FLASHLOAN_PREMIUM_TOTAL()` — never
/// hardcode this value. Pass `state.aave_status.fee_bps` from BotState.
pub fn simulate_opportunity(
    opp:          &Opportunity,
    _network:     &Network,
    base_fee:     u128,
    eth_price:    f64,
    aave_fee_bps: u64,
) -> Result<SimulationResult> {
    let flash_usd = opp.flash_amount as f64 / 1e6;
    let aave_fee  = flash_usd * (aave_fee_bps as f64 / 10_000.0);
    let gas_cost  = gas_usd_for_route(&opp.route, base_fee, eth_price);
    let gross     = opp.gross_swap_profit_usd;
    let net       = gross - aave_fee - gas_cost;

    Ok(SimulationResult {
        success:          net > 0.0,
        gross_profit_usd: gross,
        aave_fee_usd:     aave_fee,
        gas_cost_usd:     gas_cost,
        net_profit_usd:   net,
        gas_used:         gas_units_for_route(&opp.route),
        revert_reason:    None,
        borrow_amount:    opp.flash_amount,
        gas_is_measured:  false,
    })
}

pub fn simulate_batch(
    opps:         Vec<Opportunity>,
    network:      &Network,
    base_fee:     u128,
    eth_price:    f64,
    aave_fee_bps: u64,
) -> Vec<(Opportunity, SimulationResult)> {
    opps.into_par_iter()
        .filter_map(|opp| {
            match simulate_opportunity(&opp, network, base_fee, eth_price, aave_fee_bps) {
                Ok(r) if r.success => Some((opp, r)),
                Ok(_)  => None,
                Err(e) => { tracing::warn!(error = ?e, "Sim error"); None }
            }
        })
        .collect()
}

// ── Gas estimation ────────────────────────────────────────────────────────────

/// Total gas cost in USD for a route on Arbitrum One.
///
/// Arbitrum transaction cost has TWO components — both are included here:
///   1. **L2 execution** — `units × L2_base_fee`. The L2 base fee is tiny (~0.01 gwei),
///      so this term is usually cents.
///   2. **L1 data posting** — the sequencer batches calldata to Ethereum L1 and charges
///      the poster fee back to the transaction. On Arbitrum this is the DOMINANT cost.
///      Omitting it (the old model did) makes marginal trades look profitable when they
///      are not.
///
/// L2 unit calibration (Arbitrum One):
///   2-hop standard ~310k · 3-hop ~390k · 2-hop with meta ~420k.
///
/// The L1 term is an estimate; `L1_BASE_FEE_GWEI` (env, default 10) should track the
/// current Ethereum base fee. The `eth_call` validator (validation.rs) cross-checks the
/// net-profit figure against the live contract, so a stale L1 estimate is caught before
/// it causes sustained losses.
pub fn gas_usd_for_route(route: &[kingfisher_core::types::RouteHop], base_fee: u128, eth_price: f64) -> f64 {
    let units  = gas_units_for_route(route) as f64;
    let gwei   = base_fee as f64 / 1e9;
    let l2_usd = (units * gwei / 1e9) * eth_price;
    let l1_usd = arb_l1_data_fee_usd(route, eth_price);
    l2_usd + l1_usd
}

fn gas_units_for_route(route: &[kingfisher_core::types::RouteHop]) -> u64 {
    let hop_gas: u64 = route.iter()
        .map(|h| if h.is_meta { 120_000 } else { 80_000 })
        .sum();
    150_000 + hop_gas
}

/// Estimated Arbitrum L1 calldata-posting fee in USD.
///
/// Model: `executeArb()` calldata is ~200 bytes of fixed ABI overhead plus ~160 bytes
/// per hop (the `Hop` tuple). Ethereum charges ~16 gas per non-zero calldata byte; the
/// sequencer applies Brotli compression (~2×), so a 0.5 factor is used. The L1 base fee
/// is read from `L1_BASE_FEE_GWEI` (default 10 gwei) — tune it to track Ethereum gas.
fn arb_l1_data_fee_usd(route: &[kingfisher_core::types::RouteHop], eth_price: f64) -> f64 {
    let calldata_bytes = 200.0 + route.len() as f64 * 160.0;
    let l1_gwei = std::env::var("L1_BASE_FEE_GWEI")
        .ok()
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(10.0);
    let l1_gas = calldata_bytes * 16.0 * 0.5; // 16 gas/byte, ~2× compression
    (l1_gas * l1_gwei / 1e9) * eth_price
}

// ── Calldata builder (for validation eth_call) ────────────────────────────────

/// Build `executeArb()` calldata for the validation `eth_call`.
///
/// The function selector is hardcoded here because the simulation crate cannot
/// depend on the executor crate (circular dependency).
///
/// Signature: `executeArb(address,uint256,(address,int128,int128,bool,uint256)[],uint256)`
/// Selector:  `0x37cd080e`
///
/// To re-verify: `cast sig "executeArb(address,uint256,(address,int128,int128,bool,uint256)[],uint256)"`
///
/// If the function signature changes in `KingfisherArb.sol`, update this selector.
/// A mismatch causes validation eth_calls to always revert, triggering the divergence
/// circuit breaker and auto-pausing the bot.
pub fn calldata_for_validation(
    opp: &kingfisher_core::types::Opportunity,
) -> anyhow::Result<alloy::primitives::Bytes> {
    use alloy::primitives::{U256, Bytes};

    let selector = [0x37u8, 0xcd, 0x08, 0x0e];
    let mut data: Vec<u8> = selector.to_vec();
    data.extend_from_slice(&[0u8; 12]);
    data.extend_from_slice(opp.flash_token.as_slice());
    data.extend_from_slice(&U256::from(opp.flash_amount).to_be_bytes::<32>());
    data.extend_from_slice(&U256::from(128u64).to_be_bytes::<32>());
    let min_p = ((opp.simulated_profit_usd.unwrap_or(0.0) * 0.95) * 1e6) as u128;
    data.extend_from_slice(&U256::from(min_p).to_be_bytes::<32>());
    data.extend_from_slice(&U256::from(opp.route.len()).to_be_bytes::<32>());
    for hop in &opp.route {
        data.extend_from_slice(&[0u8; 12]);
        data.extend_from_slice(hop.pool.as_slice());
        let mut ib = [0u8; 32]; ib[16..].copy_from_slice(&hop.token_in_index.to_be_bytes());
        let mut jb = [0u8; 32]; jb[16..].copy_from_slice(&hop.token_out_index.to_be_bytes());
        let mut mb = [0u8; 32]; mb[31] = hop.is_meta as u8;
        data.extend_from_slice(&ib);
        data.extend_from_slice(&jb);
        data.extend_from_slice(&mb);
        let min_out = (hop.expected_out as f64 * 0.995) as u128;
        data.extend_from_slice(&U256::from(min_out).to_be_bytes::<32>());
    }
    Ok(Bytes::from(data))
}
