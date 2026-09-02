//! # Executor
//!
//! Signs and broadcasts `KingfisherArb.executeArb()` transactions to the Arbitrum
//! sequencer (see `submission.rs`), and handles admin operations (profit withdrawal).
//!
//! ## Pre-Signed Transaction Pool (future optimization)
//! `build_raw_tx()` in `submission.rs` accepts a pre-computed nonce. Caching 3
//! signed txs per block (nonces N, N+1, N+2) would eliminate the signing round-trip
//! from the hot path — worthwhile when block latency > 50ms.
//!
//! ## Calldata Cache (future optimization)
//! `encode_execute_arb()` is deterministic for the same route + block. A
//! `HashMap<(Vec<Address>, u64), Bytes>` in executor state enables sub-1ms
//! re-encoding on repeated route evaluations within the same block.
//!
//! ## Balancer V2 Zero-Fee Flash Loans (future optimization)
//! For routes where Aave liquidity is the binding constraint, wrapping the Aave
//! flash loan inside a Balancer V2 `flashLoan()` (0% fee) saves the 5 bps Aave
//! premium per trade. Requires an alternate flash-loan source in the contract.
//!
//! ## Withdrawal via Direct Transaction
//! Profit withdrawals go via plain `eth_sendRawTransaction` — they are admin
//! operations, not time-sensitive, and do not share the arb hot path.

#![allow(clippy::too_many_arguments)]
pub mod calldata;
pub mod submission;
pub mod gas;

use std::sync::Arc;
use tokio::sync::RwLock;
use anyhow::{Context, Result};
use alloy::primitives::{Bytes, Address, U256};

use kingfisher_core::{
    BotState,
    config::Network,
    types::{Opportunity, TransactionResult},
};

/// Execute the best arb opportunity by broadcasting to the Arbitrum sequencer.
pub async fn execute(
    opp:     &Opportunity,
    network: &Network,
    state:   &Arc<RwLock<BotState>>,
) -> Result<TransactionResult> {
    let (gas_limit, pool_states, current_block) = {
        let s = state.read().await;
        if !s.can_trade() {
            anyhow::bail!("Cannot trade: gas critical or bot paused");
        }
        let ps: Vec<_> = s.pool_states.values().cloned().collect();
        (s.params.gas_limit_override, ps, s.last_block)
    };

    // acquire and atomically increment the local nonce before submission.
    // Concurrent tokio::spawn execute() calls each get a unique nonce from state,
    // preventing "nonce too low" reverts when two opportunities fire within the same block.
    // If local_nonce is unset (first call), fetch from chain and initialise.
    let nonce_for_this_tx: Option<u64> = {
        // Both env vars are required for nonce management. Use explicit error logging
        // rather than silent unwrap_or_default — a missing key must surface at startup,
        // not silently degrade into a per-call fetch that can produce nonce races.
        let http_url = std::env::var("RPC_HTTP_URL").unwrap_or_default();
        let bot_key  = std::env::var("BOT_PRIVATE_KEY").unwrap_or_default();

        if http_url.is_empty() {
            tracing::error!("RPC_HTTP_URL not set — nonce manager disabled, falling back to per-call fetch");
            None
        } else if bot_key.is_empty() {
            tracing::error!("BOT_PRIVATE_KEY not set — nonce manager disabled, falling back to per-call fetch");
            None
        } else {
            let current_nonce = state.read().await.local_nonce;
            match current_nonce {
                Some(n) => {
                    // Already initialised — give this tx the current nonce and bump
                    state.write().await.local_nonce = Some(n + 1);
                    Some(n)
                }
                None => {
                    // First call — fetch from chain, cache n+1 for subsequent submissions
                    use alloy::providers::{Provider, ProviderBuilder};
                    use alloy::signers::local::PrivateKeySigner;
                    let fetch_result: Option<u64> = async {
                        let signer: PrivateKeySigner = bot_key.parse().ok()?;
                        let provider = ProviderBuilder::new()
                            .connect_http(http_url.parse().ok()?);
                        let n = provider.get_transaction_count(signer.address()).await.ok()?;
                        Some(n)
                    }.await;

                    if let Some(chain_nonce) = fetch_result {
                        state.write().await.local_nonce = Some(chain_nonce + 1);
                        Some(chain_nonce)
                    } else {
                        None // fetch failed — let submission.rs fetch it inline
                    }
                }
            }
        }
    };

    // compile-time safe ABI encoding with dynamic slippage
    let call = calldata::encode_execute_arb(opp, current_block, &pool_states)?;

    let result = submission::submit_transaction(
        opp, call, network, gas_limit, nonce_for_this_tx,
    ).await?;

    tracing::info!(
        tx_hash = result.tx_hash.as_deref().unwrap_or("pending"),
        success = result.success,
        profit  = ?result.profit_usd,
        "Execution result"
    );

    Ok(result)
}

/// Execute profit withdrawal via direct `eth_sendRawTransaction`.
///
/// Withdrawals are admin operations, not MEV opportunities. They don't need:
///   - Flashbots (no MEV protection needed)
///   - Next-block targeting (24h window is fine)
///   - Private relay (no frontrun risk — no profit exposed)
///
/// Using `eth_sendRawTransaction` directly ensures the withdrawal actually lands
/// via the standard mempool, unlike a Flashbots bundle which silently expires
/// if no builder includes it.
pub async fn check_and_execute_withdrawal(
    network: &Network,
    state:   &Arc<RwLock<BotState>>,
) -> Result<()> {
    if !state.read().await.pending_withdrawal { return Ok(()); }

    let current_block = state.read().await.last_block;

    let tokens: Vec<[u8; 20]> = match network {
        Network::Mainnet => vec![
            // Native USDC, USDT, FRAX, crvUSD
            hex::decode("af88d065e77c8cC2239327C5EDb3A432268e5831").unwrap().try_into().unwrap(),
            hex::decode("Fd086bC7CD5C481DCC9C85ebE478A1C0b69FCbb9").unwrap().try_into().unwrap(),
            hex::decode("17FC002b466eEc40DaE837Fc4bE5c67993ddBd6F").unwrap().try_into().unwrap(),
            hex::decode("498Bf2B1e120FeD3ad3D42EA2165E9b73f99C1e5").unwrap().try_into().unwrap(),
        ],
        Network::Testnet | Network::Monad => {
            tracing::warn!("Withdrawal skipped on this network — no token addresses configured");
            state.write().await.pending_withdrawal = false;
            return Ok(());
        }
    };

    // Encode withdrawProfitBatch(address[]) — selector + ABI
    let selector = [0xb8u8, 0xb5, 0x8e, 0x29];
    let mut data: Vec<u8> = selector.to_vec();
    data.extend_from_slice(&U256::from(32u64).to_be_bytes::<32>());
    data.extend_from_slice(&U256::from(tokens.len()).to_be_bytes::<32>());
    for addr in &tokens {
        data.extend_from_slice(&[0u8; 12]);
        data.extend_from_slice(addr);
    }

    let calldata = Bytes::from(data);

    // use direct RPC transaction, not Flashbots
    match send_direct_transaction(network, network.kingfisher_contract(), calldata, 200_000).await {
        Ok(tx_hash) => {
            tracing::info!(tx_hash = %tx_hash, block = current_block, "Withdrawal submitted via direct tx");
            state.write().await.pending_withdrawal = false;
        }
        Err(e) => tracing::error!(error = ?e, "Withdrawal direct tx failed"),
    }

    Ok(())
}

/// Send a transaction directly via `eth_sendRawTransaction` (not Flashbots).
/// Correct tool for admin operations: withdrawal, allowlist updates, etc.
async fn send_direct_transaction(
    network:   &Network,
    to:        Address,
    data:      Bytes,
    gas_limit: u64,
) -> Result<String> {
    use alloy::providers::{Provider, ProviderBuilder};
    use alloy::network::{EthereumWallet, TransactionBuilder};
    use alloy::rpc::types::TransactionRequest;
    use alloy::signers::local::PrivateKeySigner;

    let http_url = std::env::var("RPC_HTTP_URL").context("RPC_HTTP_URL not set")?;
    let bot_key  = std::env::var("BOT_PRIVATE_KEY").context("BOT_PRIVATE_KEY not set")?;

    // Parse without exposing key in error (matches submission.rs treatment)
    let signer: PrivateKeySigner = bot_key.parse()
        .map_err(|_| anyhow::anyhow!("BOT_PRIVATE_KEY is not a valid private key"))?;
    let wallet   = EthereumWallet::from(signer.clone());
    let provider = ProviderBuilder::new()
        .wallet(wallet)
        .connect_http(http_url.parse().context("Invalid RPC_HTTP_URL")?);

    let latest   = provider
        .get_block_by_number(alloy::eips::BlockNumberOrTag::Latest).await?
        .context("No latest block")?;
    let base_fee = latest.header.base_fee_per_gas.unwrap_or(21_000_000u64) as u128;
    let priority = crate::gas::recommend_priority_fee(base_fee, false);
    let max_fee  = base_fee.saturating_mul(2).saturating_add(priority);
    let nonce    = provider.get_transaction_count(signer.address()).await?;

    let tx = TransactionRequest::default()
        .from(signer.address())
        .to(to)
        .with_input(data)
        .with_nonce(nonce)
        .with_gas_limit(gas_limit)
        .with_max_fee_per_gas(max_fee)
        .with_max_priority_fee_per_gas(priority)
        .with_chain_id(network.chain_id());

    let pending = provider.send_transaction(tx).await?;
    let hash    = format!("{:?}", pending.tx_hash());
    tracing::info!(tx_hash = %hash, "Direct transaction submitted to mempool");
    Ok(hash)
}
