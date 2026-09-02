//! # Executor
//!
//! Signs and broadcasts `KingfisherArb.executeArb()` transactions to the Arbitrum
//! sequencer (see `submission.rs`), and handles admin operations (profit withdrawal).
//!
//! Features:
//! - Dynamic Arbitrum Timeboost priority bidding per opportunity (`timeboost.rs`)
//! - Block-scoped calldata caching to eliminate hot-path re-encoding (`calldata_cache.rs`)
//! - Pre-signed gas and nonce envelope caching to reduce RPC latency (`presigned_pool.rs`)
//! - Hop-aware gas limit budgets (2-hop vs 4-hop)
//! - Zero-fee Balancer V2 flash loan support alongside Aave V3 fallback

#![allow(clippy::too_many_arguments)]
pub mod calldata;
pub mod submission;
pub mod gas;
pub mod timeboost;
pub mod calldata_cache;
pub mod presigned_pool;

use std::sync::Arc;
use tokio::sync::RwLock;
use anyhow::{Context, Result};
use alloy::primitives::{Bytes, Address};

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
    let (
        gas_limit,
        pool_states,
        current_block,
        stress_regime,
        stress_mult,
        tb_state,
        cache_enabled,
        slippage_params,
    ) = {
        let s = state.read().await;
        if !s.can_trade() {
            anyhow::bail!("Cannot trade: gas critical or bot paused");
        }
        let ps: Vec<_> = s.pool_states.values().cloned().collect();
        let gas_lim = s.params.gas_limit_for_route(opp.route.len());
        let tb = timeboost::TimeboostMarketState {
            stress_regime: s.stress_regime,
            recent_race_loss_rate: s.recent_race_loss_rate(),
            timeboost_min_profit_usd: s.params.timeboost_min_profit_usd,
            timeboost_race_loss_threshold: s.params.timeboost_race_loss_threshold,
        };
        (
            gas_lim,
            ps,
            s.last_block,
            s.stress_regime,
            s.params.stress_priority_fee_multiplier,
            tb,
            s.params.calldata_cache_enabled,
            s.params.slippage_model.clone(),
        )
    };

    // acquire and atomically increment the local nonce before submission.
    // Concurrent tokio::spawn execute() calls each get a unique nonce from state,
    // preventing "nonce too low" reverts when two opportunities fire within the same block.
    let nonce_for_this_tx: Option<u64> = {
        let http_url = std::env::var("RPC_HTTP_URL").unwrap_or_default();
        let bot_key  = std::env::var("BOT_PRIVATE_KEY").unwrap_or_default();

        if http_url.is_empty() || bot_key.is_empty() {
            tracing::warn!("RPC_HTTP_URL or BOT_PRIVATE_KEY not set — falling back to per-call fetch");
            None
        } else {
            let current_nonce = state.read().await.local_nonce;
            match current_nonce {
                Some(n) => {
                    state.write().await.local_nonce = Some(n + 1);
                    Some(n)
                }
                None => {
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
                        None
                    }
                }
            }
        }
    };

    // Fast-path: check block-scoped calldata cache before re-encoding
    let call = if let Some(cached) = calldata_cache::global_calldata_cache().get(opp, current_block, cache_enabled) {
        cached
    } else {
        let encoded = calldata::encode_execute_arb_with_params(opp, current_block, &pool_states, &slippage_params)?;
        calldata_cache::global_calldata_cache().insert(opp, current_block, encoded.clone(), cache_enabled);
        encoded
    };

    let result = submission::submit_transaction(
        opp,
        call,
        network,
        gas_limit,
        nonce_for_this_tx,
        stress_regime,
        stress_mult,
        Some(tb_state),
        Some(state),
    ).await?;

    tracing::info!(
        tx_hash = result.tx_hash.as_deref().unwrap_or("pending"),
        success = result.success,
        profit  = ?result.profit_usd,
        stress_regime,
        gas_limit,
        "Execution result"
    );

    Ok(result)
}

/// Execute profit withdrawal via direct `eth_sendRawTransaction`.
pub async fn withdraw_profits(
    token:   Address,
    network: &Network,
    state:   &Arc<RwLock<BotState>>,
) -> Result<String> {
    {
        let mut s = state.write().await;
        if s.pending_withdrawal {
            anyhow::bail!("Withdrawal already in progress");
        }
        s.pending_withdrawal = true;
    }

    let contract = network.kingfisher_contract();
    let data = {
        use alloy::sol;
        use alloy::sol_types::SolCall;
        sol! {
            function withdrawProfit(address token) external;
        }
        withdrawProfitCall { token }.abi_encode()
    };

    let result = send_direct_transaction(contract, Bytes::from(data), network, 150_000).await;
    state.write().await.pending_withdrawal = false;
    result
}

/// Background poll to execute queued withdrawals from the bot state.
pub async fn check_and_execute_withdrawal(
    network: &Network,
    state:   &Arc<RwLock<BotState>>,
) -> Result<()> {
    if !state.read().await.pending_withdrawal { return Ok(()); }

    let current_block = state.read().await.last_block;

    let tokens: Vec<Address> = match network {
        Network::Mainnet => vec![
            "0xaf88d065e77c8cC2239327C5EDb3A432268e5831".parse()?,
            "0xFd086bC7CD5C481DCC9C85ebE478A1C0b69FCbb9".parse()?,
            "0x17FC002b466eEc40DaE837Fc4bE5c67993ddBd6F".parse()?,
            "0x498Bf2B1e120FeD3ad3D42EA2165E9b73f99C1e5".parse()?,
        ],
        Network::Testnet | Network::Monad => {
            tracing::warn!("Withdrawal skipped on this network — no token addresses configured");
            state.write().await.pending_withdrawal = false;
            return Ok(());
        }
    };

    use alloy::sol;
    use alloy::sol_types::SolCall;
    sol! {
        function withdrawProfitBatch(address[] tokens) external;
    }

    let calldata = Bytes::from(withdrawProfitBatchCall { tokens }.abi_encode());

    match send_direct_transaction(network.kingfisher_contract(), calldata, network, 200_000).await {
        Ok(tx_hash) => {
            tracing::info!(tx_hash = %tx_hash, block = current_block, "Withdrawal submitted via direct tx");
            state.write().await.pending_withdrawal = false;
        }
        Err(e) => tracing::error!(error = ?e, "Withdrawal direct tx failed"),
    }

    Ok(())
}

/// Send a transaction directly to the RPC endpoint.
pub async fn send_direct_transaction(
    to:        Address,
    data:      Bytes,
    network:   &Network,
    gas_limit: u64,
) -> Result<String> {
    use alloy::network::{EthereumWallet, TransactionBuilder};
    use alloy::providers::{Provider, ProviderBuilder};
    use alloy::rpc::types::TransactionRequest;
    use alloy::signers::local::PrivateKeySigner;

    let http_url = std::env::var("RPC_HTTP_URL").context("RPC_HTTP_URL not set")?;
    let bot_key  = std::env::var("BOT_PRIVATE_KEY").context("BOT_PRIVATE_KEY not set")?;

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
    let priority = crate::gas::recommend_priority_fee(base_fee, false, 0.25);
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
