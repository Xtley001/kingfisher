//! Priority fee calculation for Arbitrum.
//! On Arbitrum, the priority fee is typically 0 — base fee dominates.
//! Gas is ~$0.05 per flash loan arb at normal conditions.

pub fn recommend_priority_fee(base_fee: u128, stress_regime: bool, multiplier: f64) -> u128 {
    if stress_regime {
        // Tunable premium during stress (default 0.25 = 25%)
        (base_fee as f64 * multiplier.max(0.0)) as u128
    } else {
        0  // No tip needed on Arbitrum normally
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_recommend_priority_fee_normal() {
        let fee = recommend_priority_fee(100_000_000, false, 0.25);
        assert_eq!(fee, 0);
    }

    #[test]
    fn test_recommend_priority_fee_stress() {
        let fee = recommend_priority_fee(100_000_000, true, 0.25);
        assert_eq!(fee, 25_000_000);

        let fee_custom = recommend_priority_fee(100_000_000, true, 0.50);
        assert_eq!(fee_custom, 50_000_000);
    }
}
