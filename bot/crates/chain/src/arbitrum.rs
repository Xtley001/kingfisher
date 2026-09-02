//! Arbitrum One ChainAdapter Implementation
//! Wraps sequencer connection, multicall pool state fetching, and sequencer tx submission.

use alloy::primitives::{Address, TxHash};
use alloy::providers::{Provider, ProviderBuilder, WsConnect};
use anyhow::{Context, Result};
use futures::future::BoxFuture;
use futures::stream::{BoxStream, StreamExt};
use std::sync::Arc;

use crate::adapter::{ChainAdapter, ChainEvent, SignedTx, StrategyId, TxReceipt, VenueEntry};
use kingfisher_core::types::PoolState;
use kingfisher_core::venues::arbitrum as venues_arb;

pub struct ArbitrumAdapter {
    chain_id: u64,
    ws_url: String,
    http_url: String,
    venues: Vec<(StrategyId, Vec<VenueEntry>)>,
}

impl ArbitrumAdapter {
    pub fn new(chain_id: u64, ws_url: String, http_url: String) -> Self {
        let a1_venues = vec![
            VenueEntry { name: "crvUSD-USDC", address: venues_arb::crvusd_usdc_pool() },
            VenueEntry { name: "crvUSD-USDT", address: venues_arb::crvusd_usdt_pool() },
            VenueEntry { name: "FRAX-USDC", address: venues_arb::frax_usdc_pool() },
            VenueEntry { name: "2pool", address: venues_arb::twopool() },
        ];

        let a2_venues = vec![
            VenueEntry { name: "crvUSD-USDC", address: venues_arb::crvusd_usdc_pool() },
            VenueEntry { name: "crvUSD-USDT", address: venues_arb::crvusd_usdt_pool() },
        ];

        let a6_venues = vec![
            VenueEntry { name: "AaveV3Pool", address: venues_arb::aave_pool() },
        ];

        let venues = vec![
            (StrategyId::A1, a1_venues.clone()),
            (StrategyId::A2, a2_venues),
            (StrategyId::A3, a1_venues),
            (StrategyId::A6, a6_venues),
        ];

        Self {
            chain_id,
            ws_url,
            http_url,
            venues,
        }
    }
}

impl ChainAdapter for ArbitrumAdapter {
    fn chain_id(&self) -> u64 {
        self.chain_id
    }

    fn name(&self) -> &'static str {
        "arbitrum"
    }

    fn block_stream(&self) -> BoxFuture<'static, Result<BoxStream<'static, u64>>> {
        let ws_url = self.ws_url.clone();
        Box::pin(async move {
            let provider = ProviderBuilder::new()
                .connect_ws(WsConnect::new(&ws_url))
                .await
                .context("Arbitrum WS connect failed")?;
            let sub = provider.subscribe_blocks().await?;
            let stream = sub.into_stream().map(|b| b.number);
            Ok(stream.boxed())
        })
    }

    fn event_stream(&self) -> BoxFuture<'static, Result<BoxStream<'static, ChainEvent>>> {
        // Arbitrum has no public mempool; empty event stream
        Box::pin(async move {
            Ok(futures::stream::empty::<ChainEvent>().boxed())
        })
    }

    fn fetch_pool_states<'a>(&'a self, addrs: &'a [Address]) -> BoxFuture<'a, Result<Vec<PoolState>>> {
        let http_url = self.http_url.clone();
        Box::pin(async move {
            let provider = Arc::new(ProviderBuilder::new()
                .connect_http(http_url.parse().context("Invalid HTTP URL")?));
            let network = kingfisher_core::config::Network::Mainnet;
            let pools: Vec<_> = network.pools()
                .into_iter()
                .filter(|p| addrs.contains(&p.address))
                .collect();
            let block = provider.get_block_number().await?;
            let pool_states = crate::multicall::fetch_pool_states(&provider, &pools, block).await?;
            Ok(pool_states)
        })
    }

    fn submit<'a>(&'a self, tx: SignedTx) -> BoxFuture<'a, Result<TxReceipt>> {
        let http_url = self.http_url.clone();
        Box::pin(async move {
            let provider = ProviderBuilder::new()
                .connect_http(http_url.parse().context("Invalid HTTP URL")?);
            let pending = provider.send_raw_transaction(&tx.raw).await?;
            let tx_hash: TxHash = *pending.tx_hash();
            Ok(TxReceipt {
                tx_hash,
                status: true,
                block_number: None,
                gas_used: None,
            })
        })
    }

    fn venues(&self, strategy: StrategyId) -> &[VenueEntry] {
        self.venues
            .iter()
            .find_map(|(s, v)| if *s == strategy { Some(v.as_slice()) } else { None })
            .unwrap_or(&[])
    }
}
