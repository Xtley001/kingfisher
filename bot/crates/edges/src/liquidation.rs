//! Edge A6 — Generic Aave-Fork Liquidation (Arbitrum One Aave V3)
//! Ported from Peregrine's STRIKE and adapted for Arbitrum Aave V3.
//!
//! Reuses Kingfisher's canonical AAVE_POOL (0x794a61358D6845594F94dc1DB02A252b5b4814aD)
//! as both the liquidation target AND the flash loan source.

use alloy::primitives::{Address, U256};
use alloy::sol;
use serde::{Deserialize, Serialize};

use kingfisher_core::types::Opportunity;

sol! {
    interface IAaveV3Pool {
        function getUserAccountData(address user) external view returns (
            uint256 totalCollateralBase,
            uint256 totalDebtBase,
            uint256 availableBorrowsBase,
            uint256 currentLiquidationThreshold,
            uint256 ltv,
            uint256 healthFactor
        );

        function getReserveData(address asset) external view returns (
            uint256 configuration,
            uint128 liquidityIndex,
            uint128 currentLiquidityRate,
            uint128 variableBorrowIndex,
            uint128 currentVariableBorrowRate,
            uint128 currentStableBorrowRate,
            uint40  lastUpdateTimestamp,
            uint16  id,
            address aTokenAddress,
            address stableDebtTokenAddress,
            address variableDebtTokenAddress,
            address interestRateStrategyAddress,
            uint128 accruedToTreasury,
            uint128 unbacked,
            uint128 isolationModeTotalDebt
        );

        function liquidationCall(
            address collateralAsset,
            address debtAsset,
            address user,
            uint256 debtToCover,
            bool receiveAToken
        ) external;
    }
}

pub const FIRE_THRESHOLD: f64 = 1.0;
pub const PRESIGN_THRESHOLD: f64 = 1.30;
pub const DEFAULT_LIQUIDATION_BONUS: f64 = 0.05; // 5% fallback

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BorrowPosition {
    pub borrower: Address,
    pub collateral_asset: Address,
    pub debt_asset: Address,
    pub collateral_amount: U256,
    pub debt_amount: U256,
    pub collateral_usd: f64,
    pub debt_usd: f64,
    pub health_factor: f64,
    pub liquidation_bonus: f64,
    pub pre_sign_block: u64,
}

impl BorrowPosition {
    pub fn is_liquidatable(&self) -> bool {
        self.health_factor < FIRE_THRESHOLD
            && !self.debt_amount.is_zero()
            && self.collateral_asset != Address::ZERO
    }

    pub fn liquidation_profit_usd(&self) -> f64 {
        let max_cover_usd = self.debt_usd * 0.50; // Aave V3 standard 50% close factor
        max_cover_usd * self.liquidation_bonus
    }
}

/// Compute optimal flash loan size for covering up to 50% of debt
pub fn compute_liquidation_flash(debt_usd: f64) -> f64 {
    (debt_usd * 0.50).min(5_000_000.0) // 50% close factor, capped
}

/// Decode liquidation bonus from Aave V3 ReserveConfigurationMap.
/// bits 32-47 = liquidation bonus in bps (e.g. 10500 bps = 105% = 5% bonus).
pub fn decode_liquidation_bonus(config_data: U256) -> f64 {
    let bonus_bps = ((config_data >> 32u32) & U256::from(0xFFFFu64)).to::<u128>() as f64;
    if bonus_bps < 10_000.0 {
        DEFAULT_LIQUIDATION_BONUS
    } else {
        (bonus_bps / 10_000.0) - 1.0
    }
}

/// Scan a set of borrow positions for liquidatable targets.
pub fn scan_liquidation_candidates(
    positions: &[BorrowPosition],
    block: u64,
    min_profit_usd: f64,
    gas_cost_usd: f64,
) -> Vec<Opportunity> {
    let mut opps = Vec::new();

    for pos in positions {
        if !pos.is_liquidatable() {
            continue;
        }

        let gross = pos.liquidation_profit_usd();
        let net = gross - gas_cost_usd;

        if net < min_profit_usd {
            continue;
        }

        let flash_usd = compute_liquidation_flash(pos.debt_usd);
        let flash_token_decimals = 6; // USDC default; scaled per asset in production
        let flash_amount = U256::from((flash_usd * 10f64.powi(flash_token_decimals)) as u128);

        opps.push(Opportunity {
            id: uuid::Uuid::new_v4().to_string(),
            block_number: block,
            detected_at: chrono::Utc::now(),
            route: vec![],
            route_description: format!(
                "A6 Liquidation: borrower {:?} | debt=${:.0} | HF={:.3}",
                pos.borrower, pos.debt_usd, pos.health_factor
            ),
            flash_token: pos.debt_asset,
            flash_amount: flash_amount.to::<u128>(),
            gross_swap_profit_usd: gross,
            estimated_profit_usd: net,
            simulated_profit_usd: Some(net),
            aave_fee_usd: Some(flash_usd * 0.0005),
            gas_cost_usd: Some(gas_cost_usd),
            edge_trigger: Some("A6_Liquidation".into()),
            flash_source: kingfisher_core::types::FlashSource::Aave,
        });
    }

    opps.sort_by(|a, b| {
        b.estimated_profit_usd
            .partial_cmp(&a.estimated_profit_usd)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    opps
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_liquidation_bonus() {
        // 10500 bps = 105% -> 5% bonus
        let raw = U256::from(10500u64) << 32u32;
        let bonus = decode_liquidation_bonus(raw);
        assert!((bonus - 0.05).abs() < 1e-6);

        // Fallback when below 10,000 bps
        let raw_invalid = U256::from(8000u64) << 32u32;
        let fallback = decode_liquidation_bonus(raw_invalid);
        assert_eq!(fallback, DEFAULT_LIQUIDATION_BONUS);
    }

    #[test]
    fn test_scan_liquidation_candidates() {
        let borrower: Address = "0x1111111111111111111111111111111111111111".parse().unwrap();
        let collateral: Address = "0x2222222222222222222222222222222222222222".parse().unwrap();
        let debt: Address = "0x3333333333333333333333333333333333333333".parse().unwrap();

        let positions = vec![
            // Healthy position (HF 1.5) - should be skipped
            BorrowPosition {
                borrower,
                collateral_asset: collateral,
                debt_asset: debt,
                collateral_amount: U256::from(150_000e6),
                debt_amount: U256::from(100_000e6),
                collateral_usd: 150_000.0,
                debt_usd: 100_000.0,
                health_factor: 1.5,
                liquidation_bonus: 0.05,
                pre_sign_block: 100,
            },
            // Unhealthy position (HF 0.95) - liquidatable
            BorrowPosition {
                borrower,
                collateral_asset: collateral,
                debt_asset: debt,
                collateral_amount: U256::from(90_000e6),
                debt_amount: U256::from(100_000e6),
                collateral_usd: 90_000.0,
                debt_usd: 100_000.0,
                health_factor: 0.95,
                liquidation_bonus: 0.05,
                pre_sign_block: 100,
            },
        ];

        let opps = scan_liquidation_candidates(&positions, 101, 10.0, 1.0);
        assert_eq!(opps.len(), 1);
        assert_eq!(opps[0].flash_token, debt);
        // 50% of 100,000 = $50,000 flash loan
        assert_eq!(opps[0].flash_amount, 50_000_000_000); // 50,000 * 10^6
        // Gross profit = 50,000 * 0.05 = $2500
        assert_eq!(opps[0].gross_swap_profit_usd, 2500.0);
        assert_eq!(opps[0].estimated_profit_usd, 2499.0); // minus $1 gas
    }
}
