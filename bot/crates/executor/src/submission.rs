//! # Transaction Submission (Arbitrum One)
//!
//! ## Why not Flashbots / bundles?
//! Arbitrum One does **not** use Proposer-Builder Separation (PBS) and has **no
//! public mempool**. A single sequencer (Offchain Labs) orders transactions
//! first-come-first-served. There is no Flashbots relay for Arbitrum: submitting
//! `eth_sendBundle` to `relay.flashbots.net` (an Ethereum-L1-only service) does
//! nothing for chain_id 42161. There is also nobody to sandwich an atomic
//! flash-loan arb, so bundle privacy is not needed.
//!
//! The correct model on Arbitrum is therefore:
//!   1. Build an EIP-1559 transaction to `KingfisherArb.executeArb(...)`.
//!   2. Sign + broadcast it (the wallet-provider signs) to the lowest-latency
//!      sequencer endpoint via `eth_sendRawTransaction`.
//!   3. Optionally mirror the broadcast to a second fast endpoint for redundancy.
//!
//! Whichever copy the sequencer sees first wins the FCFS race. The on-chain
//! `minProfit` guard means a lost race simply reverts — capital is never at risk.
//!
//! ## Ordering priority: Arbitrum Timeboost
//! Arbitrum's Timeboost auctions a ~200ms "express lane" for priority sequencing.
//! Set `TIMEBOOST_EXPRESS_LANE_URL` to send the transaction there instead of the
//! plain sequencer RPC. Winning the auction is an off-chain process (see
//! docs/STRATEGY.md); this module only submits to the endpoint you have been granted.

use anyhow::{Context, Result};
use alloy::primitives::Bytes;
use alloy::signers::local::PrivateKeySigner;
use alloy::providers::{Provider, ProviderBuilder};
use alloy::network::{EthereumWallet, TransactionBuilder};
use alloy::rpc::types::TransactionRequest;

use kingfisher_core::{config::Network, types::{Opportunity, TransactionResult}};
use crate::gas::recommend_priority_fee;

/// Sign and broadcast the arb transaction to the Arbitrum sequencer.
///
/// Primary broadcast goes to `RPC_HTTP_URL` (the lowest-latency sequencer endpoint —
/// a co-located Nitro node forwarding to the sequencer, or `https://arb1.arbitrum.io/rpc`).
/// If `TIMEBOOST_EXPRESS_LANE_URL` is set, the primary broadcast is redirected there for
/// priority sequencing. If `SEQUENCER_BACKUP_URL` is set, a best-effort mirror broadcast
/// is fired in the background for redundancy.
///
/// `nonce_override`: caller-managed nonce to prevent races on concurrent submissions.
/// Pass `None` to fall back to an on-chain fetch (single-submit paths).
pub async fn submit_transaction(
    opp:            &Opportunity,
    calldata:       Bytes,
    network:        &Network,
    gas_limit:      u64,
    nonce_override: Option<u64>,
) -> Result<TransactionResult> {
    let bot_key  = std::env::var("BOT_PRIVATE_KEY").context("BOT_PRIVATE_KEY not set")?;
    let http_url = std::env::var("RPC_HTTP_URL").context("RPC_HTTP_URL not set")?;

    // Validate the key format BEFORE parse() so the raw key never lands in an
    // anyhow error chain (which would be written to logs).
    let signer: PrivateKeySigner = bot_key.parse()
        .map_err(|_| anyhow::anyhow!("BOT_PRIVATE_KEY is not a valid private key (check for extra whitespace, missing 0x prefix, or wrong length)"))?;

    // Priority endpoint: Timeboost express lane if configured, else the sequencer RPC.
    let primary_url = std::env::var("TIMEBOOST_EXPRESS_LANE_URL")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| http_url.clone());

    let tx = build_tx_request(&signer, network, &http_url, calldata, gas_limit, nonce_override).await?;

    tracing::info!(
        target_endpoint = %redact_url(&primary_url),
        route           = %opp.route_description,
        flash_usd       = opp.flash_amount as f64 / 1e6,
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
        success:       true,     // submitted; on-chain inclusion confirmed by LandingTracker
        profit_usd:    opp.simulated_profit_usd,
        gas_used:      None,
        revert_reason: None,
        submitted_at:  chrono::Utc::now(),
    })
}

/// Build the signed EIP-1559 `TransactionRequest` for `executeArb()`.
async fn build_tx_request(
    signer:         &PrivateKeySigner,
    network:        &Network,
    http_url:       &str,
    calldata:       Bytes,
    gas_limit:      u64,
    nonce_override: Option<u64>,
) -> Result<TransactionRequest> {
    let provider = ProviderBuilder::new()
        .connect_http(http_url.parse().context("Invalid RPC_HTTP_URL")?);

    let latest = provider
        .get_block_by_number(alloy::eips::BlockNumberOrTag::Latest)
        .await?
        .context("No latest block")?;
    let base_fee = latest.header.base_fee_per_gas.unwrap_or(21_000_000u64) as u128;
    let priority = recommend_priority_fee(base_fee, false);
    let max_fee  = base_fee.saturating_mul(2).saturating_add(priority);

    let nonce = match nonce_override {
        Some(n) => n,
        None    => provider.get_transaction_count(signer.address()).await?,
    };

    Ok(TransactionRequest::default()
        .from(signer.address())
        .to(network.kingfisher_contract())
        .with_input(calldata)
        .with_nonce(nonce)
        .with_gas_limit(gas_limit)
        .with_max_fee_per_gas(max_fee)
        .with_max_priority_fee_per_gas(priority)
        .with_chain_id(network.chain_id()))
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
