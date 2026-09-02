//! # Dynamic Arbitrum Timeboost Bidding
//!
//! Arbitrum One has a single sequencer with no public mempool or private RPCs.
//! Priority ordering is determined via the Arbitrum Timeboost express lane.
//! This module implements per-opportunity evaluation for routing transactions
//! through the express lane based on expected profit, stress regimes, and
//! competitive race-loss metrics.

use kingfisher_core::types::Opportunity;

/// Current market and bot conditions influencing the Timeboost bidding decision.
#[derive(Debug, Clone)]
pub struct TimeboostMarketState {
    pub stress_regime: bool,
    pub recent_race_loss_rate: f64,
    pub timeboost_min_profit_usd: f64,
    pub timeboost_race_loss_threshold: f64,
}

/// Outcome of the Timeboost evaluation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimeboostBid {
    pub use_express_lane: bool,
    pub reason: &'static str,
}

/// Decide per-opportunity whether to route through the Timeboost express lane.
///
/// Rules:
/// 1. If simulated profit is below `timeboost_min_profit_usd`, never use the express lane (cost control).
/// 2. If simulated profit qualifies AND `stress_regime` is active, use the express lane (high value, contested).
/// 3. If simulated profit qualifies AND recent race loss rate exceeds threshold, use the express lane (competitive pressure).
/// 4. Otherwise, use standard sequencer RPC.
pub fn should_bid_timeboost(
    opp: &Opportunity,
    current_conditions: &TimeboostMarketState,
) -> Option<TimeboostBid> {
    let profit = opp.simulated_profit_usd.unwrap_or(opp.estimated_profit_usd);

    // Rule 1: Floor check — never bid Timeboost on low-margin trades
    if profit < current_conditions.timeboost_min_profit_usd {
        return None;
    }

    // Rule 2: Stress regime trigger
    if current_conditions.stress_regime {
        return Some(TimeboostBid {
            use_express_lane: true,
            reason: "stress_regime_priority",
        });
    }

    // Rule 3: Competitive race loss pressure trigger
    if current_conditions.recent_race_loss_rate >= current_conditions.timeboost_race_loss_threshold {
        return Some(TimeboostBid {
            use_express_lane: true,
            reason: "race_loss_pressure_threshold_exceeded",
        });
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn make_test_opp(profit: f64) -> Opportunity {
        Opportunity {
            id: "test-opp-1".into(),
            block_number: 1000,
            detected_at: Utc::now(),
            route: vec![],
            route_description: "test route".into(),
            flash_token: alloy::primitives::Address::ZERO,
            flash_amount: 100_000_000_000,
            gross_swap_profit_usd: profit + 10.0,
            estimated_profit_usd: profit,
            simulated_profit_usd: Some(profit),
            aave_fee_usd: Some(5.0),
            gas_cost_usd: Some(5.0),
            edge_trigger: None,
            flash_source: kingfisher_core::types::FlashSource::Aave,
        }
    }

    #[test]
    fn test_low_profit_never_bids_timeboost() {
        let opp = make_test_opp(40.0);
        let conditions = TimeboostMarketState {
            stress_regime: true, // even during stress
            recent_race_loss_rate: 0.80,
            timeboost_min_profit_usd: 75.0,
            timeboost_race_loss_threshold: 0.25,
        };
        assert_eq!(should_bid_timeboost(&opp, &conditions), None);
    }

    #[test]
    fn test_stress_regime_triggers_timeboost() {
        let opp = make_test_opp(150.0);
        let conditions = TimeboostMarketState {
            stress_regime: true,
            recent_race_loss_rate: 0.0,
            timeboost_min_profit_usd: 75.0,
            timeboost_race_loss_threshold: 0.25,
        };
        let bid = should_bid_timeboost(&opp, &conditions);
        assert!(bid.is_some());
        assert_eq!(bid.unwrap().reason, "stress_regime_priority");
    }

    #[test]
    fn test_race_loss_pressure_triggers_timeboost() {
        let opp = make_test_opp(100.0);
        let conditions = TimeboostMarketState {
            stress_regime: false,
            recent_race_loss_rate: 0.35,
            timeboost_min_profit_usd: 75.0,
            timeboost_race_loss_threshold: 0.25,
        };
        let bid = should_bid_timeboost(&opp, &conditions);
        assert!(bid.is_some());
        assert_eq!(bid.unwrap().reason, "race_loss_pressure_threshold_exceeded");
    }

    #[test]
    fn test_normal_conditions_skips_timeboost() {
        let opp = make_test_opp(100.0);
        let conditions = TimeboostMarketState {
            stress_regime: false,
            recent_race_loss_rate: 0.10,
            timeboost_min_profit_usd: 75.0,
            timeboost_race_loss_threshold: 0.25,
        };
        assert_eq!(should_bid_timeboost(&opp, &conditions), None);
    }
}
