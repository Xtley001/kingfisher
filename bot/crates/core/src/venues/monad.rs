//! Monad Mainnet (Chain ID 143) — Venue & Protocol Registry
//! Sourced from Peregrine's deployments and Pyth documentation.

use alloy::primitives::{Address, B256};

pub const MORPHO_VAULT: &str = "0xD5D960E8c380B724a48AC59E2DfF1b2CB4a1eAee";
pub const UNISWAP_V4_PM: &str = "0x188d586ddcf52439676ca21a244753fa19f9ea8e";
pub const UNISWAP_V4_ROUTER: &str = "0x0d97dc33264bfc1c226207428a79b26757fb9dc3";
pub const PYTH_CONTRACT: &str = "0xB754BA51E3861Ac0Cb67f73CD046dE790A36508d";
pub const KURU_ROUTER: &str = "0x0d3a1BE29E9dEd63c7a5678b31e847D68F71FFa2";
pub const KURU_MARKET_FACTORY: &str = "0xd651346d7c789536ebf06dc72aE3C8502cd695CC";
pub const CRYSTAL_ROUTER: &str = "0x4e77071D619Aa164cA6427547aefA41AC51BE7A0";
pub const CRYSTAL_FACTORY: &str = "0x4e77071D619Aa164cA6427547aefA41AC51BE7A0";

pub const PYTH_FEED_ETH_USD: &str = "ff61491a931112ddf1bd8147cd1b641375f79f5825126d665480874634fd0ace";
pub const PYTH_FEED_BTC_USD: &str = "e62df6c8b4a85fe1a67db44dc12de5db330f7ac66b72dc658afedf0f4a415b43";

pub fn morpho_vault() -> Address { MORPHO_VAULT.parse().unwrap() }
pub fn uniswap_v4_pm() -> Address { UNISWAP_V4_PM.parse().unwrap() }
pub fn uniswap_v4_router() -> Address { UNISWAP_V4_ROUTER.parse().unwrap() }
pub fn pyth_contract() -> Address { PYTH_CONTRACT.parse().unwrap() }
pub fn kuru_router() -> Address { KURU_ROUTER.parse().unwrap() }
pub fn kuru_market_factory() -> Address { KURU_MARKET_FACTORY.parse().unwrap() }
pub fn crystal_router() -> Address { CRYSTAL_ROUTER.parse().unwrap() }
pub fn crystal_factory() -> Address { CRYSTAL_FACTORY.parse().unwrap() }

pub fn pyth_feed_eth_usd() -> B256 { PYTH_FEED_ETH_USD.parse().unwrap() }
pub fn pyth_feed_btc_usd() -> B256 { PYTH_FEED_BTC_USD.parse().unwrap() }
