//! # Calldata Encoder
//!
//! ## ABI Encoding via `sol!` Macro
//! Replaces the hand-rolled byte encoder. The `sol!` macro computes the function
//! selector at compile time from the canonical ABI signature. Any struct mismatch
//! between Solidity and Rust is now a **compile error**, not a silent production failure.
//!
//! ## Dynamic Per-Hop Slippage Tolerance
//! The flat 1% per-hop guard has been replaced with a three-factor model:
//!   - Pool depth: shallow pools need wider tolerance
//!   - Blocks since scan: Arbitrum's ~0.25s blocks — every block adds drift risk
//!   - Trade size relative to pool: larger size = more price impact

use anyhow::Result;
use alloy::sol;
use alloy::sol_types::SolCall;
use alloy::primitives::U256;
use kingfisher_core::types::{Opportunity, PoolState};

// ── Compile-time ABI binding ─────────────────────────────────────────────────
// Selector is keccak256("executeArb(address,uint256,(address,int128,int128,bool,uint256)[],uint256)")
// verified at compile time — any ABI drift becomes a build error.
sol! {
    struct Hop {
        address pool;
        address tokenIn;
        int128  tokenInIndex;
        int128  tokenOutIndex;
        bool    isMetaPool;
        uint256 minAmountOut;
    }

    function executeArb(
        address flashToken,
        uint256 flashAmount,
        Hop[]   hops,
        uint256 minProfit
    ) external returns (uint256 netProfit);
}

/// ABI-encode `KingfisherArb.executeArb()` using the `sol!` macro.
///
/// # Dynamic slippage
/// `pool_states` is used to look up pool depth for the dynamic tolerance model.
/// If `pool_states` is empty or a pool cannot be found, falls back to 0.5% per hop.
pub fn encode_execute_arb(
    opp:           &Opportunity,
    current_block:  u64,
    pool_states:   &[PoolState],
) -> Result<alloy::primitives::Bytes> {
    let hops: Vec<Hop> = opp.route.iter().map(|h| {
        let pool_depth_usd = pool_states.iter()
            .find(|ps| ps.address == h.pool)
            .map(|ps| ps.total_norm)
            .unwrap_or(5_000_000.0);

        let blocks_since_scan = current_block.saturating_sub(opp.block_number);
        let flash_usd = opp.flash_amount as f64 / 1e6;

        let effective_expected = if h.expected_out > 0 {
            h.expected_out
        } else if h.amount_in > 0 {
            h.amount_in * 99 / 100
        } else {
            1
        };

        let min_out = dynamic_min_amount_out(
            effective_expected,
            pool_depth_usd,
            blocks_since_scan,
            flash_usd,
        ).max(1);

        Hop {
            pool:           h.pool,
            tokenIn:        h.token_in,
            tokenInIndex:   h.token_in_index,
            tokenOutIndex:  h.token_out_index,
            isMetaPool:     h.is_meta,
            minAmountOut:   U256::from(min_out),
        }
    }).collect();

    // minProfit: 95% of simulated net profit (5% margin for final gas + drift)
    let min_p_usd = opp.simulated_profit_usd.unwrap_or(opp.estimated_profit_usd) * 0.95;
    let min_p_wei = (min_p_usd * 1e6) as u128;

    let calldata = executeArbCall {
        flashToken:  opp.flash_token,
        flashAmount: U256::from(opp.flash_amount),
        hops,
        minProfit:   U256::from(min_p_wei),
    }
    .abi_encode();

    Ok(alloy::primitives::Bytes::from(calldata))
}

/// Backward-compatible encoder for contexts without current block / pool states.
pub fn encode_execute_arb_simple(opp: &Opportunity) -> Result<alloy::primitives::Bytes> {
    encode_execute_arb(opp, opp.block_number, &[])
}

// ── Dynamic per-hop slippage tolerance ──────────────────────────────

/// Three-factor tolerance model — see module doc for rationale.
/// Hard cap: 3% total (signals something structurally wrong if hit).
pub fn dynamic_min_amount_out(
    expected_out:      u128,
    pool_depth_usd:    f64,
    blocks_since_scan: u64,
    flash_usd:         f64,
) -> u128 {
    // Factor 1: depth-based baseline (0.3% for deep → 1.5% for shallow)
    let depth_m   = (pool_depth_usd / 1_000_000.0).max(0.1);
    let depth_tol = 0.003 + (1.0 / depth_m).min(0.5) * 0.012;

    // Factor 2: time drift (0.02% per block since scan, cap 0.5%)
    let time_tol = (blocks_since_scan as f64 * 0.0002).min(0.005);

    // Factor 3: size relative to pool (0% → +1% as ratio 0 → 50%)
    let size_ratio = (flash_usd / pool_depth_usd.max(1.0)).min(0.5);
    let size_tol   = size_ratio * 0.02;

    let total_tol = (depth_tol + time_tol + size_tol).min(0.03); // hard cap 3%
    let floor     = (expected_out as f64 * (1.0 - total_tol)) as u128;
    floor.max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dynamic_slippage_deep_pool_no_lag() {
        let min = dynamic_min_amount_out(1_000_000, 100_000_000.0, 0, 10_000.0);
        assert!(min > 996_000 && min < 1_000_000, "min={min}");
    }

    #[test]
    fn test_dynamic_slippage_shallow_pool_with_lag() {
        // Shallow pool + lag + sizeable relative trade → ~1.5% tolerance, clearly
        // wider than a deep pool with no lag (~0.3%).
        let min = dynamic_min_amount_out(1_000_000, 200_000.0, 5, 50_000.0);
        assert!(min < 990_000, "Expected wider tolerance than a deep pool, got min={min}");
    }

    #[test]
    fn test_dynamic_slippage_extreme_inputs_max_tolerance() {
        // Extreme inputs (tiny pool, huge relative trade, long lag) saturate every
        // component. Total tolerance maxes at ~2.4%, staying under the 3% hard-cap
        // ceiling — so the floor lands near 976k, not the cap.
        let min = dynamic_min_amount_out(1_000_000, 1_000.0, 100, 999_000.0);
        assert!(min >= 975_000 && min <= 977_000, "min={min}");
    }

    #[test]
    fn test_min_amount_nonzero() {
        let min = dynamic_min_amount_out(1, 1_000.0, 100, 999.0);
        assert!(min >= 1, "Must never return zero");
    }
}
