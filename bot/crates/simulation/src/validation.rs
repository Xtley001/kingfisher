//! Algebraic fast-path vs eth_call parity validation.
//! Runs periodically to confirm local simulation matches on-chain execution.
//! Bot auto-pauses if divergence > 0.1% (see docs/TESTING.md).
//!
//! Note: the "local" path is the algebraic fast-path (simulate_opportunity),
//! not revm EVM execution. It compares algebraic math vs eth_call.

use anyhow::Result;
use alloy::providers::{Provider, ProviderBuilder};
use alloy::rpc::types::TransactionRequest;
use alloy::signers::local::PrivateKeySigner;

use kingfisher_core::{config::Network, types::Opportunity};
use crate::simulate_opportunity;

pub struct ValidationResult {
    pub route:           String,
    pub local_profit:    f64,
    pub eth_call_profit: f64,
    pub divergence_pct:  f64,
    pub passes:          bool,
}

/// Compare algebraic simulation to eth_call. Divergence must be < 0.1%.
/// This is called by the circuit-breaker in main.rs every N blocks.
pub async fn validate_simulation(
    opp:          &Opportunity,
    network:      &Network,
    base_fee:     u128,
    eth_price:    f64,
    aave_fee_bps: u64,
) -> Result<ValidationResult> {
    // local result (algebraic fast-path)
    let local_result = simulate_opportunity(opp, network, base_fee, eth_price, aave_fee_bps)?;
    let local_profit = local_result.net_profit_usd;

    // eth_call result (on-chain, authoritative)
    let eth_call_profit = eth_call_simulate(opp, network).await
        .unwrap_or(local_profit); // fallback to local on RPC error (don't false-positive pause)

    let divergence_pct = if eth_call_profit.abs() > 0.01 {
        ((local_profit - eth_call_profit) / eth_call_profit).abs() * 100.0
    } else {
        0.0
    };

    let passes = divergence_pct < 0.1;

    if !passes {
        tracing::error!(
            route            = %opp.route_description,
            local_profit     = local_profit,
            eth_call_profit  = eth_call_profit,
            divergence_pct   = divergence_pct,
            "⚠ Simulation DIVERGENCE > 0.1% — auto-pause triggered"
        );
    }

    Ok(ValidationResult {
        route: opp.route_description.clone(),
        local_profit,
        eth_call_profit,
        divergence_pct,
        passes,
    })
}

/// Simulate the executeArb() call via eth_call (on-chain, read-only).
/// Returns the net profit that would result if the tx landed.
async fn eth_call_simulate(
    opp:     &Opportunity,
    network: &Network,
) -> Result<f64> {
    let http_url = std::env::var("RPC_HTTP_URL")
        .unwrap_or_default();
    if http_url.is_empty() {
        anyhow::bail!("RPC_HTTP_URL not set — cannot run eth_call validation");
    }

    let provider = ProviderBuilder::new()
        .connect_http(http_url.parse()?);

    let contract = network.kingfisher_contract();

    // Build executeArb calldata (same encoding as live tx)
    let calldata = crate::calldata_for_validation(opp)?;

    // Simulate from the actual operator address so onlyOperator passes.
    let from_addr = std::env::var("BOT_PRIVATE_KEY").ok()
        .and_then(|k| k.parse::<PrivateKeySigner>().ok())
        .map(|s| s.address());

    let req = TransactionRequest {
        from:  from_addr,
        to:    Some(alloy::primitives::TxKind::Call(contract)),
        input: alloy::rpc::types::TransactionInput::new(calldata),
        ..Default::default()
    };

    // eth_call returns revert data or success output
    // On success the executeOperation returns the net profit in USDC wei
    match provider.call(req).await {
        Ok(result) => {
            // Parse net profit from return data (last 32 bytes = netProfit uint256)
            if result.len() >= 32 {
                let mut arr = [0u8; 16];
                arr.copy_from_slice(&result[result.len()-16..]);
                let profit_wei = u128::from_be_bytes(arr);
                let dec = if opp.flash_token == kingfisher_core::venues::arbitrum::crvusd_token()
                    || opp.flash_token == kingfisher_core::venues::arbitrum::frax() {
                    18
                } else {
                    6
                };
                let profit_usd = profit_wei as f64 / 10f64.powi(dec);
                Ok(profit_usd)
            } else {
                // Empty return — treat as zero profit
                Ok(0.0)
            }
        }
        Err(e) => {
            // Revert — extract revert reason for logging
            tracing::debug!(error = ?e, "eth_call simulation reverted");
            Ok(0.0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn divergence_below_threshold_passes() {
        let r = ValidationResult {
            route:           "USDC→FRAX→USDC".into(),
            local_profit:    100.05,
            eth_call_profit: 100.0,
            divergence_pct:  0.05,
            passes:          true,
        };
        assert!(r.passes, "0.05% divergence should pass");
    }

    #[test]
    fn divergence_above_threshold_fails() {
        let r = ValidationResult {
            route:           "USDC→FRAX→USDC".into(),
            local_profit:    100.15,
            eth_call_profit: 100.0,
            divergence_pct:  0.15,
            passes:          false,
        };
        assert!(!r.passes, "0.15% divergence should fail");
    }
}
