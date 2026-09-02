//! Venue Resolver — CLOB Market Address Type Safety (Fixes Gap #2)
//!
//! Enforces type distinction between token addresses and CLOB market-contract addresses.
//! A bare Address cannot be passed where a MarketAddress is required.

use alloy::primitives::Address;
use serde::{Deserialize, Serialize};

/// Type-safe wrapper around a CLOB market contract address.
/// Distinct from a bare token Address to prevent routing reverts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MarketAddress(pub Address);

impl MarketAddress {
    pub fn new(address: Address) -> Option<Self> {
        if address == Address::ZERO {
            None
        } else {
            Some(Self(address))
        }
    }

    pub fn as_address(&self) -> Address {
        self.0
    }
}

impl std::fmt::Display for MarketAddress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

/// A registered CLOB market entry mapping a trading pair to its market contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarketEntry {
    pub market_address: MarketAddress,
    pub token_in: Address,
    pub token_out: Address,
}

impl MarketEntry {
    pub fn new(market_address: MarketAddress, token_in: Address, token_out: Address) -> Self {
        Self {
            market_address,
            token_in,
            token_out,
        }
    }
}

/// Registry for CLOB markets across venues (Kuru, Crystal).
#[derive(Default, Debug, Clone)]
pub struct VenueMarketRegistry {
    markets: Vec<MarketEntry>,
}

impl VenueMarketRegistry {
    pub fn new() -> Self {
        Self { markets: Vec::new() }
    }

    pub fn register(&mut self, market: MarketAddress, token_in: Address, token_out: Address) {
        self.markets.push(MarketEntry::new(market, token_in, token_out));
    }

    /// Resolve the market contract address for a given pair.
    /// Returns None if no market is registered — NEVER falls back to token address.
    pub fn resolve_market(&self, token_in: Address, token_out: Address) -> Option<MarketAddress> {
        self.markets.iter().find_map(|m| {
            if (m.token_in == token_in && m.token_out == token_out)
                || (m.token_in == token_out && m.token_out == token_in)
            {
                Some(m.market_address)
            } else {
                None
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_market_address_type_safety() {
        let addr = "0x0d3a1BE29E9dEd63c7a5678b31e847D68F71FFa2".parse().unwrap();
        let market = MarketAddress::new(addr).expect("Valid address");
        assert_eq!(market.as_address(), addr);

        let zero_market = MarketAddress::new(Address::ZERO);
        assert!(zero_market.is_none());
    }

    #[test]
    fn test_venue_market_resolution() {
        let mut reg = VenueMarketRegistry::new();
        let market = MarketAddress("0x1111111111111111111111111111111111111111".parse().unwrap());
        let usdc: Address = "0x2222222222222222222222222222222222222222".parse().unwrap();
        let mon: Address = "0x3333333333333333333333333333333333333333".parse().unwrap();

        reg.register(market, mon, usdc);

        assert_eq!(reg.resolve_market(mon, usdc), Some(market));
        assert_eq!(reg.resolve_market(usdc, mon), Some(market));

        let other: Address = "0x4444444444444444444444444444444444444444".parse().unwrap();
        assert_eq!(reg.resolve_market(mon, other), None);
    }
}
