// Chainlink price feed helpers — used by multicall for peg monitoring
// All addresses are on Arbitrum One / Sepolia

use alloy::primitives::Address;

pub struct ChainlinkFeeds {
    pub eth_usd:  Address,
    pub usdc_usd: Address,
    pub usdt_usd: Address,
}

impl ChainlinkFeeds {
    pub fn for_arbitrum_one() -> Self {
        Self {
            eth_usd:  "0x639Fe6ab55C921f74e7fac1ee960C0B6293ba612".parse().unwrap(),
            usdc_usd: "0x50834F3163758fcC1Df9973b6e91f0F0F0434aD3".parse().unwrap(),
            usdt_usd: "0x3f3f5dF88dC9F13eac63DF89EC16ef6e7E25DdE7".parse().unwrap(),
        }
    }
}
