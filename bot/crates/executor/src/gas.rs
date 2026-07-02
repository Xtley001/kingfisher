//! Priority fee calculation for Arbitrum.
//! On Arbitrum, the priority fee is typically 0 — base fee dominates.
//! Gas is ~$0.05 per flash loan arb at normal conditions.

pub fn recommend_priority_fee(base_fee: u128, stress_regime: bool) -> u128 {
    if stress_regime {
        // During stress events, add a small tip to avoid being crowded out
        base_fee / 4  // 25% premium during stress
    } else {
        0  // No tip needed on Arbitrum normally
    }
}
