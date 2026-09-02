//! Arbitrum One (Chain ID 42161) — Venue & Protocol Registry
//! Every address here is verified against Kingfisher's canonical deployments.

use alloy::primitives::Address;

pub const AAVE_POOL: &str = "0x794a61358D6845594F94dc1DB02A252b5b4814aD";
pub const CURVE_FACTORY: &str = "0xb17b674D9c5CB2e441F8e196a2f048A81355d031";
pub const NATIVE_USDC: &str = "0xaf88d065e77c8cC2239327C5EDb3A432268e5831";
pub const USDT: &str = "0xFd086bC7CD5C481DCC9C85ebE478A1C0b69FCbb9";
pub const FRAX: &str = "0x17FC002b466eEc40DaE837Fc4bE5c67993ddBd6F";
pub const CRVUSD_TOKEN: &str = "0x498Bf2B1e120FeD3ad3D42EA2165E9b73f99C1e5";
pub const LLAMMA_WETH_CONTROLLER: &str = "0x1E0165DbD2019441aB7927C018701f3138114D71";
pub const CRVUSD_USDC_POOL: &str = "0xec090cf6DD891D2d014beA6edAda6e05E025D93d";
pub const CRVUSD_USDT_POOL: &str = "0x73aF1150F265419Ef8a5DB41908B700C32D49135";
pub const FRAX_USDC_POOL: &str = "0xC9B8a3FDECB9D5b218d02555a8Baf332E5B740d5";
pub const TWOPOOL: &str = "0x7f90122BF0700F9E7e1F688fe926940E8839F353";
pub const USDC_E: &str = "0xFF970A61A04b1cA14834A43f5dE4533eBDDB5CC8";
pub const CHAINLINK_USDC_USD: &str = "0x50834F3163758fcC1Df9973b6e91f0F0F0434aD3";
pub const CHAINLINK_USDT_USD: &str = "0x3f3f5dF88dC9F13eac63DF89EC16ef6e7E25DdE7";
pub const CHAINLINK_ETH_USD: &str = "0x639Fe6ab55C921f74e7fac1ee960C0B6293ba612";

pub fn aave_pool() -> Address { AAVE_POOL.parse().unwrap() }
pub fn curve_factory() -> Address { CURVE_FACTORY.parse().unwrap() }
pub fn native_usdc() -> Address { NATIVE_USDC.parse().unwrap() }
pub fn usdt() -> Address { USDT.parse().unwrap() }
pub fn frax() -> Address { FRAX.parse().unwrap() }
pub fn crvusd_token() -> Address { CRVUSD_TOKEN.parse().unwrap() }
pub fn llamma_weth_controller() -> Address { LLAMMA_WETH_CONTROLLER.parse().unwrap() }
pub fn crvusd_usdc_pool() -> Address { CRVUSD_USDC_POOL.parse().unwrap() }
pub fn crvusd_usdt_pool() -> Address { CRVUSD_USDT_POOL.parse().unwrap() }
pub fn frax_usdc_pool() -> Address { FRAX_USDC_POOL.parse().unwrap() }
pub fn twopool() -> Address { TWOPOOL.parse().unwrap() }
pub fn usdc_e() -> Address { USDC_E.parse().unwrap() }
pub fn chainlink_usdc_usd() -> Address { CHAINLINK_USDC_USD.parse().unwrap() }
pub fn chainlink_usdt_usd() -> Address { CHAINLINK_USDT_USD.parse().unwrap() }
pub fn chainlink_eth_usd() -> Address { CHAINLINK_ETH_USD.parse().unwrap() }
