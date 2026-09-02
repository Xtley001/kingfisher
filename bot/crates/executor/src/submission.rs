//! # Sequencer Submission & Timeboost Express Lane
//!
//! Submits transactions to the Arbitrum sequencer via HTTP RPC.
//! Dynamically evaluates opportunities for the Timeboost express lane,
//! avoiding priority overhead on small trades while securing priority
//! during stress regimes and intense competitive pressure.

use std::sync::Arc;
use tokio::sync::RwLock;
use alloy::{
    network::{EthereumWallet, TransactionBuilder},
    primitives::{Address, Bytes},
    providers::{Provider, ProviderBuilder},
    rpc::types::TransactionRequest,
    signers::local::PrivateKeySigner,
};
use anyhow::{Context, Result};
use kingfisher_core::{
    BotState,
    config::Network,
    types::{Opportunity, TransactionResult},
};

use crate::gas::recommend_priority_fee;
use crate::timeboost::{should_bid_timeboost, TimeboostMarketState};
use crate::presigned_pool::global_presigned_pool;

/// Submit an arbitrage transaction to the Arbitrum sequencer.
///
/// Dynamically routes to the Timeboost express lane if `should_bid_timeboost` returns true
/// and `TIMEBOOST_EXPRESS_LANE_URL` is set; otherwise submits to standard sequencer RPC.
pub async fn submit_transaction(
    opp:                            &Opportunity,
    calldata:                       Bytes,
    network:                        &Network,
    gas_limit:                      u64,
    nonce_override:                 Option<u64>,
    stress_regime:                  bool,
    stress_priority_fee_multiplier: f64,
    timeboost_state:                Option<TimeboostMarketState>,
    state:                          Option<&Arc<RwLock<BotState>>>,
) -> Result<TransactionResult> {
    let bot_key  = std::env::var("BOT_PRIVATE_KEY").context("BOT_PRIVATE_KEY not set")?;
    let http_url = std::env::var("RPC_HTTP_URL").context("RPC_HTTP_URL not set")?;

    let signer: PrivateKeySigner = bot_key.parse()
        .map_err(|_| anyhow::anyhow!("BOT_PRIVATE_KEY is not a valid private key"))?;

    // Evaluate Timeboost express lane bidding
    let express_url = std::env::var("TIMEBOOST_EXPRESS_LANE_URL").ok().filter(|s| !s.is_empty());
    let should_bid = timeboost_state.as_ref().and_then(|st| should_bid_timeboost(opp, st));

    let (primary_url, is_timeboost_routed) = match (should_bid, express_url) {
        (Some(bid), Some(url)) => {
            tracing::info!(reason = bid.reason, "⚡ Routing via Timeboost express lane");
            (url, true)
        }
        _ => (http_url.clone(), false),
    };

    let tx = build_tx_request(
        &signer,
        network,
        &http_url,
        opp.block_number,
        calldata,
        gas_limit,
        nonce_override,
        stress_regime,
        stress_priority_fee_multiplier,
    ).await?;

    tracing::info!(
        target_endpoint     = %redact_url(&primary_url),
        is_timeboost_routed,
        route               = %opp.route_description,
        flash_usd           = opp.flash_amount as f64 / 1e6,
        stress_regime,
        "📡 Broadcasting arb transaction to sequencer"
    );

    let tx_hash = match broadcast(&signer, &primary_url, tx.clone()).await {
        Ok(h)  => {
            tracing::debug!(tx_hash = %h, "✅ Accepted by sequencer");
            Some(h)
        }
        Err(e) => {
            tracing::warn!(error = %e, "Sequencer rejected transaction");
            return Ok(failed_result(opp, "Sequencer rejected".into()));
        }
    };

    // Track Timeboost routing metrics in state
    if let (Some(s), Some(ref h)) = (state, &tx_hash) {
        let mut w = s.write().await;
        if is_timeboost_routed {
            w.timeboost_routed_count += 1;
            w.timeboost_tx_hashes.insert(h.clone());
        } else {
            w.standard_routed_count += 1;
        }
    }

    // Best-effort mirror broadcast for redundancy (does not affect the result).
    if let Ok(backup) = std::env::var("SEQUENCER_BACKUP_URL") {
        if !backup.is_empty() {
            let (s, t) = (signer.clone(), tx.clone());
            tokio::spawn(async move {
                match broadcast(&s, &backup, t).await {
                    Ok(_)  => tracing::debug!(endpoint = %redact_url(&backup), "Mirror broadcast sent"),
                    Err(e) => tracing::debug!(error = %e, "Mirror broadcast failed (non-fatal)"),
                }
            });
        }
    }

    Ok(TransactionResult {
        id:            opp.id.clone(),
        block_target:  opp.block_number + 1,
        block_landed:  None,     // populated by LandingTracker
        tx_hash,
        success:       false,    // pending; on-chain inclusion confirmed by LandingTracker
        profit_usd:    None,     // finalized by LandingTracker from on-chain ArbExecuted event
        gas_used:      None,
        revert_reason: None,
        submitted_at:  chrono::Utc::now(),
    })
}

/// Pure helper to construct the EIP-1559 TransactionRequest, enabling unit testing of fee logic.
pub fn construct_tx_request(
    signer_address:                 Address,
    to:                             Address,
    calldata:                       Bytes,
    gas_limit:                      u64,
    nonce:                          u64,
    chain_id:                       u64,
    base_fee:                       u128,
    stress_regime:                  bool,
    stress_priority_fee_multiplier: f64,
) -> TransactionRequest {
    let priority = recommend_priority_fee(base_fee, stress_regime, stress_priority_fee_multiplier);
    let max_fee  = base_fee.saturating_mul(2).saturating_add(priority);

    TransactionRequest::default()
        .from(signer_address)
        .to(to)
        .with_input(calldata)
        .with_nonce(nonce)
        .with_gas_limit(gas_limit)
        .with_max_fee_per_gas(max_fee)
        .with_max_priority_fee_per_gas(priority)
        .with_chain_id(chain_id)
}

/// Build the signed EIP-1559 `TransactionRequest` for `executeArb()`.
async fn build_tx_request(
    signer:                         &PrivateKeySigner,
    network:                        &Network,
    http_url:                       &str,
    current_block:                  u64,
    calldata:                       Bytes,
    gas_limit:                      u64,
    nonce_override:                 Option<u64>,
    stress_regime:                  bool,
    stress_priority_fee_multiplier: f64,
) -> Result<TransactionRequest> {
    let provider = ProviderBuilder::new()
        .connect_http(http_url.parse().context("Invalid RPC_HTTP_URL")?);

    // Fast-path: check presigned gas envelope pool first
    let base_fee = if let Some(env) = global_presigned_pool().get_envelope(current_block, true) {
        env.base_fee
    } else {
        let latest = provider
            .get_block_by_number(alloy::eips::BlockNumberOrTag::Latest)
            .await?
            .context("No latest block")?;
        let fee = latest.header.base_fee_per_gas.unwrap_or(21_000_000u64) as u128;
        global_presigned_pool().update_envelope(current_block, fee);
        fee
    };

    let nonce = match nonce_override {
        Some(n) => n,
        None    => provider.get_transaction_count(signer.address()).await?,
    };

    Ok(construct_tx_request(
        signer.address(),
        network.kingfisher_contract(),
        calldata,
        gas_limit,
        nonce,
        network.chain_id(),
        base_fee,
        stress_regime,
        stress_priority_fee_multiplier,
    ))
}

/// Sign (via the wallet-provider) and broadcast a transaction to `url`, returning the hash.
async fn broadcast(signer: &PrivateKeySigner, url: &str, tx: TransactionRequest) -> Result<String> {
    let wallet   = EthereumWallet::from(signer.clone());
    let provider = ProviderBuilder::new()
        .wallet(wallet)
        .connect_http(url.parse().context("Invalid submission endpoint URL")?);
    let pending = provider.send_transaction(tx).await?;
    Ok(format!("{:?}", pending.tx_hash()))
}

/// Strip query strings (which often contain API keys) before logging an endpoint.
fn redact_url(url: &str) -> String {
    url.split('?').next().unwrap_or(url).to_string()
}

fn failed_result(opp: &Opportunity, reason: String) -> TransactionResult {
    TransactionResult {
        id:            opp.id.clone(),
        block_target:  opp.block_number + 1,
        block_landed:  None,
        tx_hash:       None,
        success:       false,
        profit_usd:    None,
        gas_used:      None,
        revert_reason: Some(reason),
        submitted_at:  chrono::Utc::now(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stress_regime_priority_fee_applied_to_tx_request() {
        let signer = Address::repeat_byte(0x11);
        let to = Address::repeat_byte(0x22);
        let base_fee = 100_000_000u128; // 0.1 gwei

        // Normal regime: priority fee is 0
        let normal_tx = construct_tx_request(
            signer, to, Bytes::new(), 500_000, 1, 42161, base_fee, false, 0.25,
        );
        assert_eq!(normal_tx.max_priority_fee_per_gas, Some(0));
        assert_eq!(normal_tx.max_fee_per_gas, Some(200_000_000));

        // Stress regime: priority fee is 25% of base fee (25_000_000)
        let stress_tx = construct_tx_request(
            signer, to, Bytes::new(), 500_000, 1, 42161, base_fee, true, 0.25,
        );
        assert_eq!(stress_tx.max_priority_fee_per_gas, Some(25_000_000));
        assert_eq!(stress_tx.max_fee_per_gas, Some(225_000_000));

        // Stress regime with custom 50% multiplier
        let stress_custom_tx = construct_tx_request(
            signer, to, Bytes::new(), 500_000, 1, 42161, base_fee, true, 0.50,
        );
        assert_eq!(stress_custom_tx.max_priority_fee_per_gas, Some(50_000_000));
        assert_eq!(stress_custom_tx.max_fee_per_gas, Some(250_000_000));
    }
}
