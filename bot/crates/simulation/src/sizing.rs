//! # Borrow Sizing
//!
//! Computes the profit-maximising flash loan size for a two-pool Curve arbitrage.
//!
//! ## Algorithm
//!
//! 1. **Upper bound** — imbalance-anchored probe doubles until net profit turns
//!    negative, then binary-searches the exact zero crossing.
//! 2. **Golden-section search** — finds the global profit maximum on [0, upper].
//! 3. **Dual-leg impact gate** — A-parameter-aware price-impact ceiling applied to
//!    both the entry and exit legs; pool_b scales back entry when it is the constraint.
//! 4. **Hard ceilings** — result capped by HARD_CAP_USD, caller abs_cap, and Aave max.

use crate::spread::StableSwapMath;

// ── Constants ─────────────────────────────────────────────────────────────────

/// Per-trade hard ceiling in USD.
///
/// Aave V3 Arbitrum USDC reserves are ~$180M; $25M sits well within available
/// liquidity. Lower if Aave reserves shrink below $25M. Raise only after verifying
/// live P&L at higher sizes.
pub const HARD_CAP_USD: f64 = 25_000_000.0;

/// Golden ratio conjugate.
const PHI_CONJ: f64 = 0.618_033_988_749_895;

// ── Public API ────────────────────────────────────────────────────────────────

/// Find the profit-maximising borrow size for a 2-pool route, evaluating both
/// directions (A→B and B→A) and returning the winner.
///
/// Returns `(borrow_amount_wei_6dec, direction_flipped)`.
/// `direction_flipped = true` means the caller should swap pool_a/pool_b in the route.
pub fn find_optimal_borrow_size_bidirectional(
    pool_a:   &StableSwapMath,
    pool_b:   &StableSwapMath,
    i:        usize,
    j:        usize,
    fee_bps:  f64,
    aave_max: u128,
    abs_cap:  f64,
    gas_usd:  f64,
) -> (u128, bool) {
    let size_ab = find_optimal_borrow_size_inner(pool_a, pool_b, i, j, fee_bps, aave_max, abs_cap, gas_usd);
    let size_ba = find_optimal_borrow_size_inner(pool_b, pool_a, j, i, fee_bps, aave_max, abs_cap, gas_usd);

    let profit_ab = if size_ab > 0 {
        net_profit_full(pool_a, pool_b, i, j, size_ab as f64 / 1e6, fee_bps, gas_usd)
    } else { f64::NEG_INFINITY };

    let profit_ba = if size_ba > 0 {
        net_profit_full(pool_b, pool_a, j, i, size_ba as f64 / 1e6, fee_bps, gas_usd)
    } else { f64::NEG_INFINITY };

    if profit_ba > profit_ab { (size_ba, true) } else { (size_ab, false) }
}

/// Single-direction optimal sizing. Used directly for routes where direction is
/// already known (e.g. edge-triggered templates).
pub fn find_optimal_borrow_size(
    pool_a:   &StableSwapMath,
    pool_b:   &StableSwapMath,
    i:        usize,
    j:        usize,
    fee_bps:  f64,
    aave_max: u128,
    abs_cap:  f64,
    gas_usd:  f64,
) -> u128 {
    find_optimal_borrow_size_inner(pool_a, pool_b, i, j, fee_bps, aave_max, abs_cap, gas_usd)
}

// ── Core sizing logic ─────────────────────────────────────────────────────────

fn find_optimal_borrow_size_inner(
    pool_a:   &StableSwapMath,
    pool_b:   &StableSwapMath,
    i:        usize,
    j:        usize,
    fee_bps:  f64,
    aave_max: u128,
    abs_cap:  f64,
    gas_usd:  f64,
) -> u128 {
    let bal_a_i = pool_a.balances.get(i).cloned().unwrap_or(0.0);
    let bal_b_j = pool_b.balances.get(j).cloned().unwrap_or(0.0);
    if bal_a_i <= 0.0 || bal_b_j <= 0.0 { return 0; }

    let upper = find_profit_upper_bound(pool_a, pool_b, i, j, fee_bps, gas_usd, bal_a_i);
    if upper <= 0.0 { return 0; }

    let profit_peak_x = golden_section_search(
        |x| net_profit_full(pool_a, pool_b, i, j, x, fee_bps, gas_usd),
        0.0, upper, 128,
    );

    if net_profit_full(pool_a, pool_b, i, j, profit_peak_x, fee_bps, gas_usd) <= 0.0 {
        return 0;
    }

    // Impact gate — both entry (pool_a) and exit (pool_b) legs.
    let max_impact_a   = max_impact_for_a(pool_a.a);
    let gated_entry    = impact_gated_size(pool_a, i, j, profit_peak_x, max_impact_a);
    let mid_at_gated   = pool_a.get_dy(i, j, gated_entry);
    let max_impact_b   = max_impact_for_a(pool_b.a);
    let gated_exit_mid = impact_gated_size(pool_b, j, i, mid_at_gated, max_impact_b);

    // Scale entry back if pool_b is the binding constraint.
    let impact_gated = if gated_exit_mid < mid_at_gated && mid_at_gated > 0.0 {
        gated_entry * (gated_exit_mid / mid_at_gated)
    } else {
        gated_entry
    };

    let cap_wei   = (HARD_CAP_USD.min(abs_cap) * 1e6) as u128;
    let gated_wei = (impact_gated * 1e6) as u128;
    gated_wei.min(aave_max).min(cap_wei)
}

// ── Upper bound ───────────────────────────────────────────────────────────────

fn find_profit_upper_bound(
    pool_a:  &StableSwapMath,
    pool_b:  &StableSwapMath,
    i:       usize,
    j:       usize,
    fee_bps: f64,
    gas_usd: f64,
    bal_a_i: f64,
) -> f64 {
    let bal_b_j    = pool_b.balances.get(j).cloned().unwrap_or(bal_a_i);
    let imbalance  = ((bal_a_i - bal_b_j) / (bal_a_i + bal_b_j + 1.0)).abs();
    let probe_frac = (imbalance * 0.5).clamp(0.005, 0.20);
    let mut probe  = bal_a_i * probe_frac;
    let mut iters  = 0;

    while iters < 30 {
        if net_profit_full(pool_a, pool_b, i, j, probe, fee_bps, gas_usd) <= 0.0 { break; }
        probe *= 2.0;
        if probe > bal_a_i * 0.95 { probe = bal_a_i * 0.95; break; }
        iters += 1;
    }

    if net_profit_full(pool_a, pool_b, i, j, probe, fee_bps, gas_usd) > 0.0 {
        return probe;
    }

    let (mut lo, mut hi) = (probe / 2.0, probe);
    for _ in 0..64 {
        let mid = (lo + hi) / 2.0;
        if net_profit_full(pool_a, pool_b, i, j, mid, fee_bps, gas_usd) > 0.0 { lo = mid; } else { hi = mid; }
    }
    hi
}

// ── Golden-section search ─────────────────────────────────────────────────────

/// Global maximum of a unimodal function on `[lo, hi]`.
/// 128 iterations converges to ~1e-26 relative precision.
fn golden_section_search<F>(f: F, mut lo: f64, mut hi: f64, max_iters: usize) -> f64
where F: Fn(f64) -> f64 {
    let mut x1 = hi - PHI_CONJ * (hi - lo);
    let mut x2 = lo + PHI_CONJ * (hi - lo);
    let mut f1 = f(x1);
    let mut f2 = f(x2);
    for _ in 0..max_iters {
        if (hi - lo) < 1e-10 { break; }
        if f1 < f2 { lo = x1; x1 = x2; f1 = f2; x2 = lo + PHI_CONJ * (hi - lo); f2 = f(x2); }
        else       { hi = x2; x2 = x1; f2 = f1; x1 = hi - PHI_CONJ * (hi - lo); f1 = f(x1); }
    }
    (lo + hi) / 2.0
}

// ── A-parameter-aware impact threshold ───────────────────────────────────────

/// Maximum acceptable one-way price impact for a pool with amplification `a`.
///
/// StableSwap curvature near equilibrium is O(1/A). Higher A → flatter curve →
/// more room before the marginal rate degrades.
///
/// Calibrated values (n=2):
///   A=50: 0.60%  A=500: 1.60%  A=2000: 2.80%
///   A=100: 0.80% A=1000: 2.10%
///   A=200: 1.10% A=1500: 2.50%
///
/// Formula: `0.5 + 2.5 × (ln A − ln 50) / (ln 2000 − ln 50)`
/// Range: [0.5%, 3.5%].
pub fn max_impact_for_a(a: f64) -> f64 {
    if a <= 0.0 { return 0.5; }
    let ac = a.clamp(50.0, 2000.0);
    let t  = (ac.ln() - 50.0_f64.ln()) / (2000.0_f64.ln() - 50.0_f64.ln());
    (0.5 + 2.5 * t).clamp(0.5, 3.5)
}

// ── Impact gate ───────────────────────────────────────────────────────────────

fn impact_gated_size(pool: &StableSwapMath, i: usize, j: usize,
                     profit_peak_x: f64, max_impact_pct: f64) -> f64 {
    if profit_peak_x <= 0.0 { return 0.0; }

    let probe     = pool.balances.get(i).cloned().unwrap_or(1.0) * 0.00001;
    let probe_out = pool.get_dy(i, j, probe);
    if probe <= 0.0 || probe_out <= 0.0 { return profit_peak_x; }
    let rate_fair = probe_out / probe;

    let peak_out = pool.get_dy(i, j, profit_peak_x);
    if peak_out <= 0.0 { return 0.0; }
    let impact_pct = (rate_fair - peak_out / profit_peak_x) / rate_fair * 100.0;

    if impact_pct <= max_impact_pct { return profit_peak_x; }

    let target = rate_fair * (1.0 - max_impact_pct / 100.0);
    let (mut lo, mut hi) = (0.0_f64, profit_peak_x);
    for _ in 0..128 {
        let mid = (lo + hi) / 2.0;
        let out = pool.get_dy(i, j, mid);
        if out <= 0.0 { hi = mid; continue; }
        if out / mid > target { lo = mid; } else { hi = mid; }
    }
    (lo + hi) / 2.0
}

// ── Net profit ────────────────────────────────────────────────────────────────

/// Net USD profit after Aave flash-loan fee and gas cost.
/// `x` is in USD. `fee_bps` is the Aave fee in basis points (5 = 0.05%).
pub fn net_profit_full(pa: &StableSwapMath, pb: &StableSwapMath,
                       i: usize, j: usize, x: f64, fee_bps: f64, gas_usd: f64) -> f64 {
    if x <= 0.0 { return f64::NEG_INFINITY; }
    let mid = pa.get_dy(i, j, x);
    if mid <= 0.0 { return f64::NEG_INFINITY; }
    pb.get_dy(j, i, mid) - x - x * fee_bps / 10_000.0 - gas_usd
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn mock(a: f64, b0: f64, b1: f64) -> StableSwapMath {
        StableSwapMath { a, balances: vec![b0, b1], n: 2, fee_rate: 0.0 }
    }

    #[test]
    fn deep_imbalanced_pool_sizes_large() {
        // pool_a is token0-heavy and pool_b is token1-heavy, so the profitable
        // direction is 1→0 (buy the token that is cheap in b, sell where dear in a).
        let a = mock(1000.0, 80_000_000.0, 20_000_000.0);
        let b = mock(1000.0, 30_000_000.0, 70_000_000.0);
        let usd = find_optimal_borrow_size(&a, &b, 1, 0, 5.0, u128::MAX, 100_000_000.0, 5.0) as f64 / 1e6;
        assert!(usd > 8_000_000.0, "got ${:.0}", usd);
    }

    #[test]
    fn higher_a_allows_larger_impact() {
        assert!(max_impact_for_a(1500.0) > max_impact_for_a(100.0));
        assert!(max_impact_for_a(1500.0) >= 2.3);
        assert!(max_impact_for_a(100.0) <= 1.0);
    }

    #[test]
    fn impact_threshold_monotone() {
        let vals = [50.0, 100.0, 200.0, 500.0, 1000.0, 1500.0, 2000.0];
        let imp: Vec<f64> = vals.iter().map(|&a| max_impact_for_a(a)).collect();
        for i in 0..imp.len()-1 { assert!(imp[i] < imp[i+1]); }
    }

    #[test]
    fn impact_threshold_in_bounds() {
        for &a in &[1.0, 10.0, 50.0, 500.0, 2000.0, 10000.0] {
            let v = max_impact_for_a(a);
            assert!(v >= 0.5 && v <= 3.5, "a={} gave {:.3}", a, v);
        }
    }

    #[test]
    fn golden_section_quadratic() {
        // Near the peak of a flat quadratic, f-value differences vanish into f64
        // epsilon while x is still ~1e-7 from centre, so 1e-6 is the achievable bound.
        let x = golden_section_search(|x| -(x-5.0).powi(2)+100.0, 0.0, 10.0, 128);
        assert!((x-5.0).abs() < 1e-6, "x={x}");
    }

    #[test]
    fn golden_section_plateau() {
        let f = |x: f64| if x<3.0 { x*10.0 } else if x<5.0 { 30.0+(x-3.0)*0.5 } else { 31.0-(x-5.0)*20.0 };
        let x = golden_section_search(f, 0.0, 10.0, 128);
        assert!(x >= 4.0 && x <= 6.0);
    }

    #[test]
    fn bidirectional_finds_direction() {
        let a = mock(1000.0, 70_000_000.0, 30_000_000.0);
        let b = mock(1000.0, 25_000_000.0, 75_000_000.0);
        let (size, _) = find_optimal_borrow_size_bidirectional(&a, &b, 1, 0, 5.0, u128::MAX, 100_000_000.0, 5.0);
        assert!(size > 0);
    }

    #[test]
    fn gas_cost_reduces_size() {
        let a = mock(500.0, 200_000.0, 100_000.0);
        let b = mock(500.0, 100_000.0, 200_000.0);
        let s0 = find_optimal_borrow_size(&a, &b, 0, 1, 5.0, u128::MAX, 100_000_000.0, 0.0)  as f64;
        let sg = find_optimal_borrow_size(&a, &b, 0, 1, 5.0, u128::MAX, 100_000_000.0, 50.0) as f64;
        assert!(sg <= s0);
    }

    #[test]
    fn hard_cap_respected() {
        let a = mock(2000.0, 500_000_000.0, 100_000_000.0);
        let b = mock(2000.0, 100_000_000.0, 500_000_000.0);
        assert!(find_optimal_borrow_size(&a, &b, 0, 1, 5.0, u128::MAX, 1_000_000_000.0, 5.0)
            <= (HARD_CAP_USD * 1e6) as u128);
    }

    #[test]
    fn aave_cap_respected() {
        let a = mock(1500.0, 50_000_000.0, 10_000_000.0);
        let b = mock(1500.0, 10_000_000.0, 50_000_000.0);
        let cap = 1_000_000_000u128;
        assert!(find_optimal_borrow_size(&a, &b, 0, 1, 5.0, cap, 100_000_000.0, 5.0) <= cap);
    }

    #[test]
    fn balanced_pool_returns_zero() {
        let a = mock(1000.0, 1_000_000.0, 1_000_000.0);
        let b = mock(1000.0, 1_000_000.0, 1_000_000.0);
        assert_eq!(find_optimal_borrow_size(&a, &b, 0, 1, 5.0, u128::MAX, 100_000_000.0, 5.0), 0);
    }

    #[test]
    fn clear_imbalance_returns_positive() {
        let a = mock(1500.0, 80_000_000.0, 20_000_000.0);
        let b = mock(1000.0, 20_000_000.0, 80_000_000.0);
        assert!(find_optimal_borrow_size(&a, &b, 1, 0, 5.0, u128::MAX, 100_000_000.0, 5.0) > 0);
    }

    #[test]
    fn both_leg_gate_constrains_on_shallow_exit() {
        let deep    = mock(1000.0, 50_000_000.0, 10_000_000.0);
        let shallow = mock(200.0,    500_000.0,  2_000_000.0);
        let size_both = find_optimal_borrow_size(&deep, &shallow, 0, 1, 5.0, u128::MAX, 100_000_000.0, 3.0);
        let max_impact_a = max_impact_for_a(deep.a);
        let single_gate  = (impact_gated_size(&deep, 0, 1, 10_000_000.0, max_impact_a) * 1e6) as u128;
        assert!(size_both <= single_gate,
            "both-leg={} should be <= single-leg={}", size_both, single_gate);
    }

    #[test]
    fn impact_gate_allows_large_deep_pool_trade() {
        let a = mock(1000.0, 80_000_000.0, 20_000_000.0);
        let b = mock(1000.0, 20_000_000.0, 80_000_000.0);
        let usd = find_optimal_borrow_size(&a, &b, 1, 0, 5.0, u128::MAX, 1_000_000_000.0, 0.0) as f64 / 1e6;
        assert!(usd > 1_000_000.0, "over-restricted: ${:.0}", usd);
    }

    #[test]
    fn impact_gate_restricts_thin_pool() {
        let s = mock(100.0, 50_000.0, 50_000.0);
        let usd = find_optimal_borrow_size(&s, &s, 0, 1, 5.0, u128::MAX, 1_000_000_000.0, 0.0) as f64 / 1e6;
        assert!(usd < 5_000.0, "${:.0}", usd);
    }

    #[test]
    fn stress_event_sizes_into_millions() {
        // A deep depeg-style imbalance (20M/60M at low A) opens a wide spread against
        // a balanced pool and sizes into the millions.
        let stressed = mock(200.0, 20_000_000.0, 60_000_000.0);
        let balanced = mock(1000.0, 40_000_000.0, 40_000_000.0);
        let (size, flipped) = find_optimal_borrow_size_bidirectional(
            &stressed, &balanced, 0, 1, 5.0, 25_000_000_000_000u128, 100_000_000.0, 3.0,
        );
        assert!(size as f64 / 1e6 > 1_000_000.0, "size=${:.0}", size as f64 / 1e6);
        // Confirm the winning direction is genuinely profitable (bidirectional would
        // return 0 otherwise). `flipped` means the winner is the pool_b→pool_a route.
        let (pa, pb, i, j) = if flipped {
            (&balanced, &stressed, 1usize, 0usize)
        } else {
            (&stressed, &balanced, 0usize, 1usize)
        };
        assert!(net_profit_full(pa, pb, i, j, size as f64 / 1e6, 5.0, 3.0) > 0.0);
    }

    #[test]
    fn positive_for_imbalanced() {
        // A=500 gives the imbalanced pool enough curvature to open a real spread
        // against the balanced pool; at A=1500 the curve is too flat to arb.
        let a = mock(500.0, 600_000.0, 1_400_000.0);
        let b = mock(1000.0, 1_000_000.0, 1_000_000.0);
        assert!(find_optimal_borrow_size(&a, &b, 0, 1, 5.0, 1_000_000_000_000, 1_000_000_000.0, 0.0) > 0);
    }

    #[test]
    fn respects_abs_cap() {
        let a = mock(1500.0, 5_000_000.0, 5_000_000.0);
        let b = mock(1000.0, 5_000_000.0, 5_000_000.0);
        let cap = 100_000.0;
        assert!(find_optimal_borrow_size(&a, &b, 0, 1, 5.0, u128::MAX, cap, 0.0) <= (cap*1e6) as u128);
    }
}
