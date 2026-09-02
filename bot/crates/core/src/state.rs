//! # BotState
//!
//! Central mutable state for the bot, wrapped in `Arc<RwLock<BotState>>`.
//!
//! ## Key design notes
//!
//! - **Revert classification** — `RevertClass` separates race losses (`ProfitBelowMin`)
//!   from genuine errors (`PoolNotAllowed`, `ZeroInput`). Race losses do not trip the
//!   consecutive-revert circuit breaker.
//!
//! - **Daily stat reset** — `tick_daily_reset()` must be called once per block at the
//!   top of the block handler to keep `today_profit_usd` / `today_trades` accurate.
//!
//! - **Nonce cache** — `local_nonce` prevents concurrent bundle submissions from racing
//!   on the same nonce. Refreshed from chain after any nonce-related revert.

use std::collections::{HashMap, VecDeque};
use chrono::{DateTime, NaiveDate, Utc};
use alloy::primitives::Address;
use serde::{Deserialize, Serialize};

use crate::config::{Network, BotParams};
use crate::types::{AaveReserveStatus, Opportunity, OpportunityEvent, PoolState, TransactionResult};

// ── Revert classification ───────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RevertClass {
    /// Lost the race to a competitor — normal, don't count toward consecutive_reverts
    ProfitBelowMin,
    /// Pool address not in allowlist — config bug, alert immediately
    PoolNotAllowed,
    /// Pool virtual price dropped — investigate before trading
    PoolUnhealthy,
    /// Zero input at a hop — calldata encoding bug, alert immediately
    ZeroInput,
    /// Curve minAmountOut guard hit — consider widening dynamic slippage
    SlippageGuard,
    /// Any other revert — investigate
    Unknown(String),
}

impl RevertClass {
    pub fn from_reason(reason: &str) -> Self {
        // Decode 4-byte custom error selectors if present (e.g. from on-chain revert data)
        let trimmed = reason.trim().trim_start_matches("0x");
        if trimmed.len() >= 8 {
            if let Ok(bytes) = alloy::hex::decode(&trimmed[..8]) {
                let sel: &[u8] = &bytes;
                if sel == &alloy::primitives::keccak256("ProfitBelowMin(uint256,uint256)")[..4] {
                    return Self::ProfitBelowMin;
                } else if sel == &alloy::primitives::keccak256("PoolNotAllowed(address)")[..4] {
                    return Self::PoolNotAllowed;
                } else if sel == &alloy::primitives::keccak256("PoolUnhealthy(address)")[..4] {
                    return Self::PoolUnhealthy;
                } else if sel == &alloy::primitives::keccak256("ZeroInputAtHop(uint256)")[..4] {
                    return Self::ZeroInput;
                } else if sel == &alloy::primitives::keccak256("ZeroMinAmountOut(uint256)")[..4] {
                    return Self::SlippageGuard;
                }
            }
        }

        let r = reason.to_lowercase();
        if r.contains("profit below min") || r.contains("kf: profit") {
            Self::ProfitBelowMin
        } else if r.contains("pool not allowed") {
            Self::PoolNotAllowed
        } else if r.contains("pool unhealthy") || r.contains("virtual price") {
            Self::PoolUnhealthy
        } else if r.contains("zero input") {
            Self::ZeroInput
        } else if r.contains("minamountout") || r.contains("slippage") || r.contains("exchange") {
            Self::SlippageGuard
        } else {
            Self::Unknown(reason.to_string())
        }
    }

    /// True = immediate Telegram alert; pause consecutive-revert circuit breaker
    pub fn is_critical(&self) -> bool {
        matches!(self, Self::PoolNotAllowed | Self::ZeroInput)
    }

    /// True = expected race loss; do NOT count toward consecutive_reverts
    pub fn is_race_loss(&self) -> bool {
        matches!(self, Self::ProfitBelowMin)
    }
}

// ── BotState ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotState {
    // Identity
    pub network:     Network,
    pub running:     bool,
    pub uptime_secs: u64,
    pub started_at:  DateTime<Utc>,

    // Chain state
    pub last_block:    u64,
    pub last_block_at: Option<DateTime<Utc>>,
    /// Most recent block base fee in wei. Updated every block by the chain layer.
    /// Used by the template builder for live gas estimates.
    pub last_base_fee:  u128,
    pub eth_price_usd: f64,

    // Aave V3 reserve (queried every cycle)
    pub aave_status: AaveReserveStatus,

    // Peg state (Chainlink)
    pub usdc_peg:      f64,
    pub usdt_peg:      f64,
    pub stress_regime: bool,
    /// Set when stress_regime transitions to true; cleared on resolution
    pub stress_entered_at: Option<DateTime<Utc>>,

    // ── Performance counters ──────────────────────────────────────────────────
    pub total_trades:        u64,
    pub total_reverts:       u64,  // All-time failures (never resets)
    pub total_profit_usd:    f64,
    pub win_rate:            f64,
    pub consecutive_reverts: u32,  // Only non-race-loss reverts; resets on success

    // Daily stats — reset at UTC midnight via tick_daily_reset().
    pub today_trades:      u64,
    pub today_profit_usd:  f64,
    pub day_started:       NaiveDate,

    pub revert_counts: HashMap<String, u64>,  // RevertClass key → count
    pub race_losses:   u64,  // ProfitBelowMin only
    pub error_reverts: u64,  // All non-race-loss reverts

    pub pending_bundle_hashes: Vec<(String, u64, f64)>, // (tx_hash, target_block, sim_profit)
    /// Real Opportunity structs keyed by tx_hash for divergence validation.
    /// Avoids constructing a synthetic zero-address opportunity that always reverts eth_call.
    pub pending_opportunities: std::collections::HashMap<String, Opportunity>,

    // Gas
    pub wallet_eth_balance:  f64,
    pub total_gas_spent_usd: f64,
    pub gas_regime:          GasRegime,

    // Live tunable parameters
    pub params: BotParams,

    // Feeds (last 100 each)
    pub recent_opps: VecDeque<OpportunityEvent>,
    pub recent_txs:  VecDeque<TransactionResult>,

    // Pool states (keyed by address)
    pub pool_states: HashMap<Address, PoolState>,

    // Internal control signals
    pub pending_withdrawal: bool,

    // Local nonce cache — prevents concurrent submissions from racing on the same nonce.
    // None = not yet initialised; refreshed from chain after any nonce-related revert.
    pub local_nonce: Option<u64>,

    // Timeboost vs standard express lane routing & landing metrics
    pub timeboost_routed_count: u64,
    pub standard_routed_count:  u64,
    pub timeboost_landed_count: u64,
    pub standard_landed_count:  u64,
    pub recent_race_losses:     VecDeque<bool>,
    pub timeboost_tx_hashes:    std::collections::HashSet<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum GasRegime {
    Normal,   // wallet >= alert_gas_eth
    Alert,    // wallet < alert_gas_eth — send Telegram, continue trading
    Critical, // wallet < gas_reserve_eth — bot halts
}

impl BotState {
    pub fn new(network: Network, params: BotParams) -> Self {
        Self {
            network,
            running: true,
            uptime_secs: 0,
            started_at: Utc::now(),
            last_block: 0,
            last_block_at: None,
            last_base_fee: 100_000_000, // 0.1 gwei conservative default until first block
            eth_price_usd: 0.0,
            aave_status: AaveReserveStatus::default(),
            usdc_peg: 1.0,
            usdt_peg: 1.0,
            stress_regime: false,
            stress_entered_at: None,
            total_trades: 0,
            total_reverts: 0,
            total_profit_usd: 0.0,
            win_rate: 0.0,
            consecutive_reverts: 0,
            today_trades: 0,
            today_profit_usd: 0.0,
            day_started: Utc::now().date_naive(),
            revert_counts: HashMap::new(),
            race_losses: 0,
            error_reverts: 0,
            pending_bundle_hashes: Vec::new(),
            pending_opportunities: std::collections::HashMap::new(),
            wallet_eth_balance: 0.0,
            total_gas_spent_usd: 0.0,
            gas_regime: GasRegime::Normal,
            params,
            recent_opps: VecDeque::with_capacity(100),
            recent_txs:  VecDeque::with_capacity(100),
            pool_states: HashMap::new(),
            pending_withdrawal: false,
            local_nonce: None,
            timeboost_routed_count: 0,
            standard_routed_count: 0,
            timeboost_landed_count: 0,
            standard_landed_count: 0,
            recent_race_losses: VecDeque::with_capacity(30),
            timeboost_tx_hashes: std::collections::HashSet::new(),
        }
    }

    pub fn update_gas_regime(&mut self) {
        self.gas_regime = if self.wallet_eth_balance < self.params.gas_reserve_eth {
            GasRegime::Critical
        } else if self.wallet_eth_balance < self.params.alert_gas_eth {
            GasRegime::Alert
        } else {
            GasRegime::Normal
        };
    }

    pub fn can_trade(&self) -> bool {
        self.running && self.gas_regime != GasRegime::Critical
    }

    pub fn push_opportunity(&mut self, event: OpportunityEvent) {
        if self.recent_opps.len() >= 100 { self.recent_opps.pop_front(); }
        self.recent_opps.push_back(event);
    }

    pub fn push_tx_result(&mut self, result: TransactionResult) {
        if result.success {
            self.total_trades    += 1;
            self.today_trades    += 1;
            self.consecutive_reverts = 0;
            if let Some(profit) = result.profit_usd {
                self.total_profit_usd += profit;
                self.today_profit_usd += profit;
            }
        } else {
            // Classify the revert — race losses don't trip the circuit breaker.
            let class = result.revert_reason.as_deref()
                .map(RevertClass::from_reason)
                .unwrap_or(RevertClass::Unknown("no reason".into()));

            let key = format!("{:?}", class);
            *self.revert_counts.entry(key).or_insert(0) += 1;

            if class.is_race_loss() {
                self.race_losses += 1;
                // Race losses do NOT count toward consecutive_reverts circuit breaker
            } else {
                self.error_reverts += 1;
                self.consecutive_reverts += 1;
            }
            self.total_reverts += 1;
        }

        // Win rate uses all-time totals — never resets.
        let total = self.total_trades + self.total_reverts;
        self.win_rate = if total > 0 {
            self.total_trades as f64 / total as f64
        } else {
            0.0
        };

        if self.recent_txs.len() >= 100 { self.recent_txs.pop_front(); }
        self.recent_txs.push_back(result);
    }

    /// Calculate the fraction of recent outcomes that were race losses (0.0 to 1.0).
    pub fn recent_race_loss_rate(&self) -> f64 {
        if self.recent_race_losses.is_empty() {
            return 0.0;
        }
        let losses = self.recent_race_losses.iter().filter(|&&loss| loss).count();
        losses as f64 / self.recent_race_losses.len() as f64
    }

    /// Register a bundle as pending on-chain confirmation.
    /// Also stores the real Opportunity so divergence validation can use it
    /// instead of a synthetic zero-address stub.
    pub fn register_pending_bundle(&mut self, tx_hash: String, target_block: u64, sim_profit: f64, opp: Opportunity) {
        self.pending_bundle_hashes.push((tx_hash.clone(), target_block, sim_profit));
        self.pending_opportunities.insert(tx_hash, opp);
    }

    /// Remove a bundle's stored opportunity on landing or expiry.
    pub fn remove_pending_opportunity(&mut self, tx_hash: &str) {
        self.pending_opportunities.remove(tx_hash);
    }

    /// Called by the landing tracker when an on-chain receipt is confirmed as successful (status == 1).
    /// Authoritatively records profit, trade counts, gas spent, and updates recent transactions.
    pub fn confirm_trade_landed(
        &mut self,
        tx_hash: &str,
        landed_block: u64,
        actual_profit_usd: f64,
        gas_used: u64,
        gas_cost_usd: f64,
    ) -> TransactionResult {
        self.total_gas_spent_usd += gas_cost_usd;

        if self.timeboost_tx_hashes.remove(tx_hash) {
            self.timeboost_landed_count += 1;
        } else {
            self.standard_landed_count += 1;
        }

        if self.recent_race_losses.len() >= 20 {
            self.recent_race_losses.pop_front();
        }
        self.recent_race_losses.push_back(false);

        let res = TransactionResult {
            id: tx_hash.to_string(),
            block_target: landed_block,
            block_landed: Some(landed_block),
            tx_hash: Some(tx_hash.to_string()),
            success: true,
            profit_usd: Some(actual_profit_usd),
            gas_used: Some(gas_used),
            revert_reason: None,
            submitted_at: Utc::now(),
        };

        self.push_tx_result(res.clone());
        tracing::info!(actual_profit_usd, gas_cost_usd, landed_block, tx_hash, "✅ On-chain confirmed trade landed");
        res
    }

    /// Called by the landing tracker when an on-chain transaction reverted (status == 0).
    /// Records failure, classifies revert, trips circuit breaker if not a race loss, and tracks gas spent.
    pub fn confirm_trade_reverted(
        &mut self,
        tx_hash: &str,
        landed_block: u64,
        reason: Option<String>,
        gas_used: u64,
        gas_cost_usd: f64,
    ) -> TransactionResult {
        self.total_gas_spent_usd += gas_cost_usd;
        self.timeboost_tx_hashes.remove(tx_hash);

        let is_race_loss = reason.as_deref().map(RevertClass::from_reason) == Some(RevertClass::ProfitBelowMin);
        if self.recent_race_losses.len() >= 20 {
            self.recent_race_losses.pop_front();
        }
        self.recent_race_losses.push_back(is_race_loss);

        let res = TransactionResult {
            id: tx_hash.to_string(),
            block_target: landed_block,
            block_landed: Some(landed_block),
            tx_hash: Some(tx_hash.to_string()),
            success: false,
            profit_usd: Some(0.0),
            gas_used: Some(gas_used),
            revert_reason: reason,
            submitted_at: Utc::now(),
        };

        self.push_tx_result(res.clone());
        tracing::warn!(gas_cost_usd, landed_block, tx_hash, "❌ On-chain transaction reverted");
        res
    }

    /// Called when a pending bundle expires without on-chain inclusion.
    pub fn confirm_trade_dropped(&mut self, tx_hash: &str, target_block: u64) -> TransactionResult {
        let res = TransactionResult {
            id: tx_hash.to_string(),
            block_target: target_block,
            block_landed: None,
            tx_hash: Some(tx_hash.to_string()),
            success: false,
            profit_usd: Some(0.0),
            gas_used: None,
            revert_reason: Some("Bundle expired (not included)".into()),
            submitted_at: Utc::now(),
        };

        self.push_tx_result(res.clone());
        tracing::debug!(tx_hash, target_block, "Bundle not included (expired)");
        res
    }

    /// Call once per block. Resets today_* counters at UTC midnight and logs yesterday's totals.
    pub fn tick_daily_reset(&mut self) {
        let today = Utc::now().date_naive();
        if today != self.day_started {
            tracing::info!(
                yesterday_profit = self.today_profit_usd,
                yesterday_trades = self.today_trades,
                "🌅 Daily stats reset"
            );
            self.today_profit_usd = 0.0;
            self.today_trades     = 0;
            self.day_started      = today;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Network, BotParams};

    fn state() -> BotState {
        BotState::new(Network::Testnet, BotParams::default())
    }

    fn ok_tx() -> TransactionResult {
        TransactionResult {
            id: "1".into(), block_target: 1, block_landed: None,
            tx_hash: None, success: true, profit_usd: Some(100.0),
            gas_used: None, revert_reason: None,
            submitted_at: Utc::now(),
        }
    }

    fn fail_tx(reason: &str) -> TransactionResult {
        TransactionResult {
            id: "2".into(), block_target: 2, block_landed: None,
            tx_hash: None, success: false, profit_usd: None,
            gas_used: None, revert_reason: Some(reason.into()),
            submitted_at: Utc::now(),
        }
    }

    #[test]
    fn test_race_loss_does_not_increment_consecutive_reverts() {
        let mut s = state();
        s.push_tx_result(fail_tx("KF: profit below min"));
        assert_eq!(s.consecutive_reverts, 0, "Race loss must not trip circuit breaker");
        assert_eq!(s.race_losses, 1);
        assert_eq!(s.error_reverts, 0);
    }

    #[test]
    fn test_bug_revert_increments_consecutive() {
        let mut s = state();
        s.push_tx_result(fail_tx("pool not allowed"));
        assert_eq!(s.consecutive_reverts, 1);
        assert_eq!(s.error_reverts, 1);
        assert_eq!(s.race_losses, 0);
    }

    #[test]
    fn test_success_resets_consecutive() {
        let mut s = state();
        s.push_tx_result(fail_tx("pool not allowed"));
        s.push_tx_result(fail_tx("pool not allowed"));
        s.push_tx_result(ok_tx());
        assert_eq!(s.consecutive_reverts, 0);
        assert_eq!(s.total_trades, 1);
    }

    #[test]
    fn test_daily_reset_clears_today_counters() {
        let mut s = state();
        s.today_profit_usd = 500.0;
        s.today_trades     = 5;
        // Simulate a day having passed
        s.day_started = Utc::now().date_naive()
            .pred_opt().unwrap_or(Utc::now().date_naive());
        s.tick_daily_reset();
        assert_eq!(s.today_profit_usd, 0.0, "today profit must reset");
        assert_eq!(s.today_trades, 0, "today trades must reset");
    }

    #[test]
    fn test_total_profit_not_affected_by_daily_reset() {
        let mut s = state();
        s.push_tx_result(ok_tx());
        s.total_profit_usd = 200.0;
        s.day_started      = Utc::now().date_naive().pred_opt().unwrap_or(s.day_started);
        s.tick_daily_reset();
        assert_eq!(s.total_profit_usd, 200.0, "all-time profit must survive reset");
    }

    #[test]
    fn test_win_rate_uses_all_time_reverts() {
        let mut s = state();
        s.push_tx_result(ok_tx());
        s.push_tx_result(fail_tx("pool not allowed"));
        assert!((s.win_rate - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_gas_regime_transitions() {
        let mut s = state();
        s.wallet_eth_balance = 1.0; s.update_gas_regime();
        assert_eq!(s.gas_regime, GasRegime::Normal);
        s.wallet_eth_balance = 0.20; s.update_gas_regime();
        assert_eq!(s.gas_regime, GasRegime::Alert);
        s.wallet_eth_balance = 0.05; s.update_gas_regime();
        assert_eq!(s.gas_regime, GasRegime::Critical);
    }

    #[test]
    fn test_custom_error_selector_decoding() {
        // ProfitBelowMin(uint256,uint256) selector
        let sel = alloy::primitives::keccak256("ProfitBelowMin(uint256,uint256)");
        let hex_err = format!("0x{}", alloy::hex::encode(&sel[..4]));
        let class = RevertClass::from_reason(&hex_err);
        assert_eq!(class, RevertClass::ProfitBelowMin);
        assert!(class.is_race_loss());

        // PoolNotAllowed(address) selector
        let sel2 = alloy::primitives::keccak256("PoolNotAllowed(address)");
        let hex_err2 = format!("0x{}", alloy::hex::encode(&sel2[..4]));
        let class2 = RevertClass::from_reason(&hex_err2);
        assert_eq!(class2, RevertClass::PoolNotAllowed);
        assert!(class2.is_critical());
    }

    #[test]
    fn test_confirm_trade_landed_accounting() {
        let mut s = state();
        assert_eq!(s.total_trades, 0);
        assert_eq!(s.total_profit_usd, 0.0);
        assert_eq!(s.total_gas_spent_usd, 0.0);

        let res = s.confirm_trade_landed("0xabc", 100, 42.5, 300_000, 1.25);
        assert!(res.success);
        assert_eq!(res.profit_usd, Some(42.5));
        assert_eq!(s.total_trades, 1);
        assert_eq!(s.today_trades, 1);
        assert_eq!(s.total_profit_usd, 42.5);
        assert_eq!(s.today_profit_usd, 42.5);
        assert_eq!(s.total_gas_spent_usd, 1.25);
        assert_eq!(s.consecutive_reverts, 0);
        assert_eq!(s.recent_txs.len(), 1);
    }

    #[test]
    fn test_confirm_trade_reverted_accounting() {
        let mut s = state();
        let res = s.confirm_trade_reverted("0xdef", 101, Some("pool not allowed".into()), 250_000, 0.85);
        assert!(!res.success);
        assert_eq!(s.total_trades, 0);
        assert_eq!(s.total_reverts, 1);
        assert_eq!(s.error_reverts, 1);
        assert_eq!(s.consecutive_reverts, 1);
        assert_eq!(s.total_gas_spent_usd, 0.85);
        assert_eq!(s.total_profit_usd, 0.0);
    }
}
