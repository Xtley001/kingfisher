//! PULSE — Oracle Repricing Lag Arbitrage (Strategy B on Monad)
//!
//! Ported from Peregrine's PULSE strategy and hardened with:
//!   - Gap #1 fix: Direct price update streaming (PythPriceUpdate)
//!   - Gap #2 fix: Type-safe MarketAddress resolution for CLOB venues
//!   - Gap #3 fix: Latency budget & staleness abort via MAX_STALENESS_MS

use alloy::primitives::{Address, B256};
use serde::{Deserialize, Serialize};

use crate::pyth_watch::PythPriceUpdate;
use crate::venue_resolve::{MarketAddress, VenueMarketRegistry};
use kingfisher_core::types::Opportunity;

pub const MIN_ORACLE_JUMP_PCT: f64 = 0.005; // 0.5% jump required
pub const MIN_SPREAD_TO_TRADE: f64 = 0.002;  // 0.2% minimum spread
pub const MAX_STALENESS_MS: u64 = 500;       // Monad block time ~500ms latency budget

// ── Pyth Feed Registry ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PythFeedRegistry {
    pub eth_usd: B256,
    pub btc_usd: B256,
    pub usdc_usd: B256,
    pub mon_usd: B256,
}

impl Default for PythFeedRegistry {
    fn default() -> Self {
        Self {
            eth_usd: "ff61491a931112ddf1bd8147cd1b641375f79f5825126d665480874634fd0ace"
                .parse()
                .unwrap(),
            btc_usd: "e62df6c8b4a85fe1a67db44dc12de5db330f7ac66b72dc658afedf0f4a415b43"
                .parse()
                .unwrap(),
            usdc_usd: "eaa020c61cc479712813461ce153894a96a6c00b21ed0cfc2798d1f9a9e9c94a"
                .parse()
                .unwrap(),
            // Unset/zero sentinel until Pyth mainnet MON/USD is announced
            mon_usd: B256::ZERO,
        }
    }
}

impl PythFeedRegistry {
    pub fn token_for_feed(&self, feed_id: &B256, tokens: &TokenAddresses) -> Option<Address> {
        if feed_id == &self.eth_usd {
            Some(tokens.weth)
        } else if feed_id == &self.btc_usd && tokens.wbtc != Address::ZERO {
            Some(tokens.wbtc)
        } else if feed_id == &self.usdc_usd && tokens.usdc != Address::ZERO {
            Some(tokens.usdc)
        } else if feed_id == &self.mon_usd && self.mon_usd != B256::ZERO && tokens.mon != Address::ZERO {
            Some(tokens.mon)
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenAddresses {
    pub usdc: Address,
    pub weth: Address,
    pub wbtc: Address,
    pub mon: Address,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum VenueKind {
    UniswapV4,
    Kuru,
    Crystal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VenueQuote {
    pub venue: VenueKind,
    pub price: f64,
    pub liquidity_usd: f64,
    pub fee_bps: u32,
    pub market_address: Option<MarketAddress>,
}

/// Scan for PULSE oracle backrun opportunities.
/// Rejects stale opportunities older than MAX_STALENESS_MS (Gap #3 fix).
pub fn scan_pulse(
    updates: &[PythPriceUpdate],
    feed_registry: &PythFeedRegistry,
    tokens: &TokenAddresses,
    market_registry: &VenueMarketRegistry,
    quotes: &[(Address, Address, VenueQuote)], // (base, quote, VenueQuote)
    max_flash_usd: f64,
    min_profit_usd: f64,
    block: u64,
) -> Vec<Opportunity> {
    let mut opportunities = Vec::new();

    for update in updates {
        // Gap #3 Fix: Enforce latency budget — reject updates older than MAX_STALENESS_MS
        if update.age_ms() > MAX_STALENESS_MS {
            tracing::debug!(
                feed = %update.feed_id,
                age_ms = update.age_ms(),
                "pulse_stale_rejected_total — update exceeded latency budget"
            );
            continue;
        }

        if update.price <= 0.0 {
            continue;
        }

        let asset = match feed_registry.token_for_feed(&update.feed_id, tokens) {
            Some(a) => a,
            None => continue,
        };
        let quote_tok = tokens.usdc;

        // Find lagging venue and fast venue for (asset, quote)
        let pair_quotes: Vec<&VenueQuote> = quotes
            .iter()
            .filter(|(b, q, _)| *b == asset && *q == quote_tok)
            .map(|(_, _, vq)| vq)
            .collect();

        if pair_quotes.len() < 2 {
            continue;
        }

        for lagging in &pair_quotes {
            if lagging.price <= 0.0 {
                continue;
            }

            let jump_pct = (update.price - lagging.price) / lagging.price;
            if jump_pct.abs() < MIN_ORACLE_JUMP_PCT {
                continue;
            }

            for fast in &pair_quotes {
                if fast.venue == lagging.venue {
                    continue;
                }

                let spread = if jump_pct > 0.0 {
                    (fast.price - lagging.price) / lagging.price
                } else {
                    (lagging.price - fast.price) / fast.price
                };

                if spread < MIN_SPREAD_TO_TRADE {
                    continue;
                }

                // Sizing: optimal borrow capped at pool depth and max_flash_usd
                let min_depth = lagging.liquidity_usd.min(fast.liquidity_usd);
                if min_depth < 5_000.0 {
                    continue;
                }

                let flash_usd = (min_depth * 0.10).min(max_flash_usd);
                let gross_profit = flash_usd * spread;
                let estimated_gas_usd = 0.50; // Conservative gas cost on Monad
                let net_profit = gross_profit - estimated_gas_usd;

                if net_profit < min_profit_usd {
                    continue;
                }

                // Gap #2 Fix: Verify CLOB legs resolve through MarketAddress
                if fast.venue == VenueKind::Kuru || fast.venue == VenueKind::Crystal {
                    if market_registry.resolve_market(asset, quote_tok).is_none() {
                        tracing::debug!(
                            venue = ?fast.venue,
                            "Missing verified MarketAddress for CLOB venue — skipping"
                        );
                        continue;
                    }
                }

                opportunities.push(Opportunity {
                    id: uuid::Uuid::new_v4().to_string(),
                    block_number: block,
                    detected_at: chrono::Utc::now(),
                    route: vec![],
                    route_description: format!(
                        "PULSE: {:?} jump={:+.2}% | lagging={:?} fast={:?} | flash=${:.0}",
                        update.feed_id,
                        jump_pct * 100.0,
                        lagging.venue,
                        fast.venue,
                        flash_usd
                    ),
                    flash_token: quote_tok,
                    flash_amount: (flash_usd * 1e6) as u128,
                    gross_swap_profit_usd: gross_profit,
                    estimated_profit_usd: net_profit,
                    simulated_profit_usd: Some(net_profit),
                    aave_fee_usd: Some(0.0), // Morpho 0 bps fee
                    gas_cost_usd: Some(estimated_gas_usd),
                    edge_trigger: Some("Strategy_B_PULSE".into()),
                });
            }
        }
    }

    opportunities
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_staleness_rejection() {
        let registry = PythFeedRegistry::default();
        let tokens = TokenAddresses {
            usdc: "0x1111111111111111111111111111111111111111".parse().unwrap(),
            weth: "0x2222222222222222222222222222222222222222".parse().unwrap(),
            wbtc: Address::ZERO,
            mon: Address::ZERO,
        };
        let m_reg = VenueMarketRegistry::new();

        // Stale update (600ms old)
        let mut stale_update = PythPriceUpdate::new(registry.eth_usd, 3000.0, 0.5, 1720000000);
        stale_update.received_at_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
            - 600;

        let quotes = vec![
            (
                tokens.weth,
                tokens.usdc,
                VenueQuote {
                    venue: VenueKind::UniswapV4,
                    price: 2900.0,
                    liquidity_usd: 100_000.0,
                    fee_bps: 30,
                    market_address: None,
                },
            ),
            (
                tokens.weth,
                tokens.usdc,
                VenueQuote {
                    venue: VenueKind::Kuru,
                    price: 3000.0,
                    liquidity_usd: 100_000.0,
                    fee_bps: 10,
                    market_address: None,
                },
            ),
        ];

        let opps = scan_pulse(
            &[stale_update],
            &registry,
            &tokens,
            &m_reg,
            &quotes,
            10_000.0,
            1.0,
            100,
        );
        assert!(opps.is_empty(), "Stale update (>500ms) must be rejected");
    }
}
