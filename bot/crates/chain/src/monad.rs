//! Monad ChainAdapter Implementation
//! Wraps Monad ~500ms block stream, Pyth update stream, and fast sync submission.

use alloy::consensus::Transaction;
use alloy::primitives::{Address, TxHash};
use alloy::providers::{Provider, ProviderBuilder, WsConnect};
use anyhow::{Context, Result};
use futures::future::BoxFuture;
use futures::stream::{BoxStream, StreamExt};

use crate::adapter::{ChainAdapter, ChainEvent, SignedTx, StrategyId, TxReceipt, VenueEntry};
use kingfisher_core::types::PoolState;
use kingfisher_core::venues::monad as venues_monad;

pub struct MonadAdapter {
    chain_id: u64,
    ws_url: String,
    http_url: String,
    venues: Vec<(StrategyId, Vec<VenueEntry>)>,
}

impl MonadAdapter {
    pub fn new(chain_id: u64, ws_url: String, http_url: String) -> Self {
        let b1_venues = vec![
            VenueEntry { name: "UniswapV4_PM", address: venues_monad::uniswap_v4_pm() },
            VenueEntry { name: "UniswapV4_Router", address: venues_monad::uniswap_v4_router() },
            VenueEntry { name: "KuruRouter", address: venues_monad::kuru_router() },
            VenueEntry { name: "CrystalRouter", address: venues_monad::crystal_router() },
        ];

        let venues = vec![(StrategyId::B1, b1_venues)];

        Self {
            chain_id,
            ws_url,
            http_url,
            venues,
        }
    }
}

impl ChainAdapter for MonadAdapter {
    fn chain_id(&self) -> u64 {
        self.chain_id
    }

    fn name(&self) -> &'static str {
        "monad"
    }

    fn block_stream(&self) -> BoxFuture<'static, Result<BoxStream<'static, u64>>> {
        let ws_url = self.ws_url.clone();
        Box::pin(async move {
            let provider = ProviderBuilder::new()
                .connect_ws(WsConnect::new(&ws_url))
                .await
                .context("Monad WS connect failed")?;
            let sub = provider.subscribe_blocks().await?;
            let stream = sub.into_stream().map(|b| b.number);
            Ok(stream.boxed())
        })
    }

    fn event_stream(&self) -> BoxFuture<'static, Result<BoxStream<'static, ChainEvent>>> {
        let ws_url = self.ws_url.clone();
        let pyth_contract = venues_monad::pyth_contract();

        Box::pin(async move {
            let provider = ProviderBuilder::new()
                .connect_ws(WsConnect::new(&ws_url))
                .await
                .context("Monad WS connect failed for event stream")?;

            let pending = provider.subscribe_full_pending_transactions().await?;
            let stream = pending.into_stream().filter_map(move |tx| {
                let to_addr = tx.to();
                let input_data = tx.input();
                if to_addr == Some(pyth_contract) && input_data.len() >= 4 {
                    let updates = kingfisher_oracle_lag::decode_pyth_accumulator_update(input_data);
                    if let Some(first) = updates.into_iter().next() {
                        return futures::future::ready(Some(ChainEvent::PythUpdate {
                            feed_id: first.feed_id,
                            price: first.price,
                            confidence: first.confidence,
                            publish_time: first.publish_time,
                        }));
                    }
                }
                futures::future::ready(None)
            });

            Ok(stream.boxed())
        })
    }

    fn fetch_pool_states<'a>(&'a self, _addrs: &'a [Address]) -> BoxFuture<'a, Result<Vec<PoolState>>> {
        Box::pin(async move {
            // For Monad, prices and depths are updated via indexers / price graph
            Ok(Vec::new())
        })
    }

    fn submit<'a>(&'a self, tx: SignedTx) -> BoxFuture<'a, Result<TxReceipt>> {
        let http_url = self.http_url.clone();
        Box::pin(async move {
            let provider = ProviderBuilder::new()
                .connect_http(http_url.parse().context("Invalid HTTP URL")?);

            // Fast path: eth_sendRawTransactionSync if supported, falls back to standard
            let tx_hash = match provider
                .raw_request::<_, TxHash>("eth_sendRawTransactionSync".into(), (&tx.raw,))
                .await
            {
                Ok(h) => h,
                Err(_) => {
                    let pending = provider.send_raw_transaction(&tx.raw).await?;
                    *pending.tx_hash()
                }
            };

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
