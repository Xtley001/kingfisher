use alloy::primitives::Address;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use crate::config::TokenConfig;

// ─── Pool State ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolState {
    pub address:         Address,
    pub name:            String,
    pub tokens:          Vec<TokenConfig>,
    pub balances_raw:    Vec<u128>,
    pub balances_norm:   Vec<f64>,
    pub total_norm:      f64,
    pub a_parameter:     u64,
    pub virtual_price:   u128,
    pub is_meta:         bool,
    pub balance_history: VecDeque<(u64, Vec<f64>)>,
    pub last_updated:    u64,
    /// On-chain Curve pool fee. None = use default 0.0004 (0.04%). F-10.
    pub fee_rate:        Option<f64>,
}

impl PoolState {
    /// 0.0 (balanced) → 0.5 (completely one-sided)
    pub fn imbalance_ratio(&self) -> f64 {
        if self.balances_norm.is_empty() || self.total_norm == 0.0 { return 0.0; }
        let expected = 1.0 / self.balances_norm.len() as f64;
        self.balances_norm.iter()
            .map(|&b| (b / self.total_norm - expected).abs())
            .fold(0.0_f64, f64::max)
    }

    /// Rate of change per block — freshness indicator
    pub fn velocity(&self) -> f64 {
        let hist = &self.balance_history;
        if hist.len() < 2 { return 0.0; }
        let (b_new, vals_new) = hist.back().unwrap();
        let (b_old, vals_old) = hist.front().unwrap();
        let elapsed = (*b_new - *b_old) as f64;
        if elapsed == 0.0 || vals_new.is_empty() || vals_old.is_empty() { return 0.0; }
        let s_new: f64 = vals_new.iter().sum();
        let s_old: f64 = vals_old.iter().sum();
        if s_new == 0.0 || s_old == 0.0 { return 0.0; }
        (vals_new[0] / s_new - vals_old[0] / s_old).abs() / elapsed
    }

    /// virtual_price must stay >= 1e18 (circuit breaker)
    pub fn is_healthy(&self) -> bool {
        self.virtual_price >= 1_000_000_000_000_000_000u128
    }

    /// Probe exchange rate using StableSwap D formula (inline, no dep on simulation crate)
    pub fn exchange_rate(&self, i: usize, j: usize) -> f64 {
        if i >= self.balances_norm.len() || j >= self.balances_norm.len() || i == j {
            return 0.0;
        }
        let dx = self.balances_norm[i] * 0.001;
        if dx == 0.0 { return 0.0; }
        let dy = stable_swap_get_dy(&self.balances_norm, self.a_parameter as f64, i, j, dx);
        if dx > 0.0 { dy / dx } else { 0.0 }
    }
}

/// Inline minimal StableSwap get_dy — avoids circular dep on simulation crate
fn stable_swap_get_dy(balances: &[f64], a: f64, i: usize, j: usize, dx: f64) -> f64 {
    let n = balances.len() as f64;
    let ann = a * n.powi(balances.len() as i32);
    let sum: f64 = balances.iter().sum();
    if sum == 0.0 { return 0.0; }
    let mut d = sum;
    for _ in 0..255 {
        let mut dp = d;
        for &b in balances {
            if b == 0.0 { return 0.0; }
            dp = dp * d / (b * n);
        }
        let d_prev = d;
        let num = (ann * sum + dp * n) * d;
        let den = (ann - 1.0) * d + (n + 1.0) * dp;
        if den == 0.0 { break; }
        d = num / den;
        // F-13: relative convergence
        if d > 0.0 && (d - d_prev).abs() / d <= 1e-8 { break; }
    }
    // compute y
    let x = balances[i] + dx;
    let mut s = x;
    let mut c = d;
    for (k, &b) in balances.iter().enumerate() {
        if k == j { continue; }
        let bk = if k == i { x } else { b };
        if bk == 0.0 { return 0.0; }
        s += bk;
        c = c * d / (bk * n);
    }
    c = c * d / (ann * n);
    let bv = s + d / ann;
    let mut y = d;
    for _ in 0..255 {
        let yp = y;
        let den = 2.0 * y + bv - d;
        if den == 0.0 { break; }
        y = (y * y + c) / den;
        // F-13: relative convergence
        if y > 0.0 && (y - yp).abs() / y <= 1e-8 { break; }
    }
    let dy = balances[j] - y - 1.0;
    if dy <= 0.0 { 0.0 } else { dy }
}

// ─── Aave Reserve Status ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AaveReserveStatus {
    pub available_liquidity:  u128,
    pub borrow_cap:           u128,
    pub reserve_active:       bool,
    pub last_updated_block:   u64,
    /// Flash loan premium in basis points, read from `FLASHLOAN_PREMIUM_TOTAL()`.
    /// Refreshed every 1000 blocks. Default 5 bps (current Aave V3 value) but
    /// must be read at runtime — Aave governance can change this.
    pub fee_bps:              u64,
    pub last_fee_read_block:  u64,
}

impl AaveReserveStatus {
    pub fn max_borrowable(&self) -> u128 {
        if !self.reserve_active { return 0; }
        self.available_liquidity.min(self.borrow_cap)
    }

    /// Returns the fee_bps, defaulting to 5 if the on-chain value has not yet been read.
    /// Callers should always prefer this over accessing `fee_bps` directly.
    pub fn effective_fee_bps(&self) -> u64 {
        if self.fee_bps == 0 { 5 } else { self.fee_bps }
    }
}

// ─── Opportunity ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Opportunity {
    pub id:                   String,
    pub block_number:         u64,
    pub detected_at:          DateTime<Utc>,
    pub route:                Vec<RouteHop>,
    pub route_description:    String,
    pub flash_token:          Address,
    pub flash_amount:         u128,
    pub gross_swap_profit_usd: f64,      // raw swap output minus flash amount (pre-fee)
    pub estimated_profit_usd: f64,       // gross minus aave_fee minus dynamic gas (L3 gate)
    pub simulated_profit_usd: Option<f64>,
    pub aave_fee_usd:         Option<f64>,
    pub gas_cost_usd:         Option<f64>,
    pub edge_trigger:         Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteHop {
    pub pool:            Address,
    pub pool_name:       String,
    pub token_in_index:  i128,
    pub token_out_index: i128,
    pub is_meta:         bool,
    pub amount_in:       u128,
    pub expected_out:    u128,
}

// ─── Transaction Result ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionResult {
    pub id:            String,
    pub block_target:  u64,
    pub block_landed:  Option<u64>,
    pub tx_hash:       Option<String>,
    pub success:       bool,
    pub profit_usd:    Option<f64>,
    pub gas_used:      Option<u64>,
    pub revert_reason: Option<String>,
    pub submitted_at:  DateTime<Utc>,
}

// ─── OpportunityEvent (dashboard feed) ──────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpportunityEvent {
    pub id:                   String,
    pub route_description:    String,
    pub gross_swap_profit_usd: f64,      // raw swap output minus flash amount (pre-fee)
    pub estimated_profit_usd: f64,       // gross minus aave_fee minus dynamic gas (L3 gate)
    pub simulated_profit_usd: Option<f64>,
    pub edge_trigger:         Option<String>,
    pub detected_at:          DateTime<Utc>,
    pub fired:                bool,
}

// ─── Partial BotParams update (API PATCH) ───────────────────────────────────

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BotParamsPatch {
    pub min_profit_usd:    Option<f64>,
    pub min_gas_roi:       Option<f64>,
    pub min_imbalance_pct: Option<f64>,
    pub min_velocity:      Option<f64>,
    pub gas_reserve_eth:   Option<f64>,
    pub alert_gas_eth:     Option<f64>,
    pub abs_cap_usd:       Option<f64>,
}
