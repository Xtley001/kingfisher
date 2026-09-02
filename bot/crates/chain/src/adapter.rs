//! ChainAdapter Trait — Universal Multi-Chain Abstraction Boundary
//!
//! Enables Arbitrum and Monad to be driven by a single unified executor and runtime,
//! while isolating chain-specific transport, mempool feeds, and submission pathways.

use alloy::primitives::{Address, Bytes, TxHash};
use anyhow::Result;
use futures::future::BoxFuture;
use futures::stream::BoxStream;
use serde::{Deserialize, Serialize};

use kingfisher_core::types::PoolState;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum StrategyId {
    A1, // Stablecoin depeg
    A2, // LLAMMA soft liquidation
    A3, // Large LP removal
    A4, // Gauge vote window
    A6, // Aave V3 lending liquidation
    B1, // Monad PULSE oracle repricing lag
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VenueEntry {
    pub name: &'static str,
    pub address: Address,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ChainEvent {
    PythUpdate {
        feed_id: alloy::primitives::B256,
        price: f64,
        confidence: f64,
        publish_time: u64,
    },
    PoolSwap {
        pool: Address,
        sender: Address,
        amount_in: u128,
        amount_out: u128,
    },
    Empty,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedTx {
    pub raw: Bytes,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TxReceipt {
    pub tx_hash: TxHash,
    pub status: bool,
    pub block_number: Option<u64>,
    pub gas_used: Option<u64>,
}

pub trait ChainAdapter: Send + Sync {
    /// Chain ID — used for tx signing and as config lookup key.
    fn chain_id(&self) -> u64;

    /// Human-readable name for logs/dashboard — e.g. "arbitrum", "monad".
    fn name(&self) -> &'static str;

    /// Subscribe to new blocks. Yields block number on every new block.
    fn block_stream(&self) -> BoxFuture<'static, Result<BoxStream<'static, u64>>>;

    /// Subscribe to mempool/update-stream events.
    /// Arbitrum: returns empty stream (no public mempool).
    /// Monad: Pyth Hermes / mempool update stream.
    fn event_stream(&self) -> BoxFuture<'static, Result<BoxStream<'static, ChainEvent>>>;

    /// Fetch current state for a batch of pools/markets via multicall.
    fn fetch_pool_states<'a>(&'a self, addrs: &'a [Address]) -> BoxFuture<'a, Result<Vec<PoolState>>>;

    /// Submit a signed transaction via this chain's fastest path.
    /// Arbitrum: eth_sendRawTransaction to sequencer.
    /// Monad: eth_sendRawTransactionSync.
    fn submit<'a>(&'a self, tx: SignedTx) -> BoxFuture<'a, Result<TxReceipt>>;

    /// This chain's registered venues for a given strategy module.
    fn venues(&self, strategy: StrategyId) -> &[VenueEntry];
}
