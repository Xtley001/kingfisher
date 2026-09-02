use alloy::primitives::Address;
use serde::{Deserialize, Serialize};
use crate::venues;

/// Resolve the persistent data directory used for `params.json` and `trades.jsonl`.
///
/// `KINGFISHER_DATA_DIR` if set; else `/var/lib/kingfisher` when it exists (the
/// bare-metal default — see deploy/kingfisher.service); else the current directory (dev).
pub fn data_dir() -> String {
    if let Ok(d) = std::env::var("KINGFISHER_DATA_DIR") {
        if !d.is_empty() { return d; }
    }
    if std::path::Path::new("/var/lib/kingfisher").exists() {
        return "/var/lib/kingfisher".to_string();
    }
    ".".to_string()
}

// ─── Network ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Network {
    Testnet,  // Arbitrum Sepolia — chain_id 421614
    Mainnet,  // Arbitrum One    — chain_id 42161
    Monad,    // Monad Mainnet   — chain_id 143
}

impl Network {
    pub fn from_env() -> Self {
        match std::env::var("NETWORK").as_deref() {
            Ok("monad") => {
                tracing::info!("🟣 Network: MONAD MAINNET — chain 143");
                Network::Monad
            }
            Ok("mainnet") => {
                tracing::info!("🔴 Network: ARBITRUM MAINNET — real funds");
                Network::Mainnet
            }
            _ => {
                tracing::info!("🟡 Network: TESTNET (default)");
                Network::Testnet
            }
        }
    }

    pub fn chain_id(&self) -> u64 {
        match self {
            Network::Testnet => 421_614,
            Network::Mainnet => 42_161,
            Network::Monad   => 143,
        }
    }

    pub fn aave_pool(&self) -> Address {
        match self {
            Network::Testnet => std::env::var("AAVE_POOL_ADDR").ok().filter(|s| !s.is_empty()).and_then(|s| s.parse().ok()).unwrap_or_else(|| "0xBfC91D59fdAA134A4ED45f7B584cAf96D7792Eff".parse().unwrap()),
            Network::Mainnet => venues::arbitrum::aave_pool(),
            Network::Monad   => Address::ZERO,
        }
    }

    pub fn kingfisher_contract(&self) -> Address {
        let key = match self {
            Network::Testnet => "CONTRACT_ADDRESS_TESTNET",
            Network::Mainnet => "CONTRACT_ADDRESS_MAINNET",
            Network::Monad   => "CONTRACT_ADDRESS_MONAD",
        };
        std::env::var(key)
            .unwrap_or_else(|_| panic!("{} not set in .env", key))
            .parse()
            .expect("Invalid contract address")
    }

    pub fn usdc_address(&self) -> Address {
        match self {
            Network::Testnet => std::env::var("USDC_ADDR").ok().filter(|s| !s.is_empty()).and_then(|s| s.parse().ok()).unwrap_or_else(|| "0x75faf114eafb1BDbe2F0316DF893fd58CE46AA4d".parse().unwrap()),
            Network::Mainnet => venues::arbitrum::native_usdc(),
            Network::Monad   => std::env::var("USDC").unwrap_or_default().parse().unwrap_or(Address::ZERO),
        }
    }

    pub fn chainlink_eth_usd(&self) -> Address {
        match self {
            Network::Testnet => std::env::var("CHAINLINK_ETH_USD_ADDR").ok().filter(|s| !s.is_empty()).and_then(|s| s.parse().ok()).unwrap_or_else(|| "0xd30e2101a97dcbAeBCBC04F14C3f624E67A35165".parse().unwrap()),
            Network::Mainnet => venues::arbitrum::chainlink_eth_usd(),
            Network::Monad   => Address::ZERO,
        }
    }

    pub fn chainlink_usdc_usd(&self) -> Address {
        match self {
            Network::Testnet => std::env::var("CHAINLINK_USDC_USD_ADDR").ok().filter(|s| !s.is_empty()).and_then(|s| s.parse().ok()).unwrap_or_else(|| "0x0153002d20B96532C639313c2d54c3dA09109309".parse().unwrap()),
            Network::Mainnet => venues::arbitrum::chainlink_usdc_usd(),
            Network::Monad   => Address::ZERO,
        }
    }

    pub fn chainlink_usdt_usd(&self) -> Address {
        match self {
            Network::Testnet => std::env::var("CHAINLINK_USDT_USD_ADDR").ok().filter(|s| !s.is_empty()).and_then(|s| s.parse().ok()).unwrap_or_else(|| "0x0a023a3423D9b27A0BE48c768CCF2dD7877fEf5E".parse().unwrap()),
            Network::Mainnet => venues::arbitrum::chainlink_usdt_usd(),
            Network::Monad   => Address::ZERO,
        }
    }

    pub fn curve_factory(&self) -> Option<Address> {
        match self {
            Network::Testnet => None,
            Network::Mainnet => Some(venues::arbitrum::curve_factory()),
            Network::Monad   => None,
        }
    }

    pub fn is_mainnet(&self) -> bool {
        matches!(self, Network::Mainnet | Network::Monad)
    }

    pub fn pools(&self) -> Vec<PoolConfig> {
        match self {
            Network::Mainnet => mainnet_pools(),
            Network::Testnet => testnet_pools(),
            Network::Monad   => vec![],
        }
    }
}

// ─── Pool / Token config ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenConfig {
    pub symbol:   String,
    pub address:  Address,
    pub decimals: u8,
    pub index:    usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolConfig {
    pub name:    String,
    pub address: Address,
    pub tokens:  Vec<TokenConfig>,
    pub is_meta: bool,
    /// 1 = always watch; 2 = stress-regime only
    pub priority: u8,
}

fn mainnet_pools() -> Vec<PoolConfig> {
    vec![
        PoolConfig {
            name:    "FRAX-USDC".into(),
            address: venues::arbitrum::frax_usdc_pool(),
            tokens: vec![
                TokenConfig {
                    symbol:   "FRAX".into(),
                    address:  venues::arbitrum::frax(),
                    decimals: 18,
                    index:    0,
                },
                TokenConfig {
                    symbol:   "USDC.e".into(),
                    address:  venues::arbitrum::usdc_e(),
                    decimals: 6,
                    index:    1,
                },
            ],
            is_meta: false,
            priority: 1,
        },
        PoolConfig {
            name:    "crvUSD-USDC".into(),
            address: venues::arbitrum::crvusd_usdc_pool(),
            tokens: vec![
                TokenConfig {
                    symbol:   "crvUSD".into(),
                    address:  venues::arbitrum::crvusd_token(),
                    decimals: 18,
                    index:    0,
                },
                TokenConfig {
                    symbol:   "USDC".into(),
                    address:  venues::arbitrum::native_usdc(),
                    decimals: 6,
                    index:    1,
                },
            ],
            is_meta: false,
            priority: 1,
        },
        PoolConfig {
            name:    "crvUSD-USDT".into(),
            address: venues::arbitrum::crvusd_usdt_pool(),
            tokens: vec![
                TokenConfig {
                    symbol:   "crvUSD".into(),
                    address:  venues::arbitrum::crvusd_token(),
                    decimals: 18,
                    index:    0,
                },
                TokenConfig {
                    symbol:   "USDT".into(),
                    address:  venues::arbitrum::usdt(),
                    decimals: 6,
                    index:    1,
                },
            ],
            is_meta: false,
            priority: 1,
        },
        PoolConfig {
            name:    "2pool".into(),
            address: venues::arbitrum::twopool(),
            tokens: vec![
                TokenConfig {
                    symbol:   "USDC.e".into(),
                    address:  venues::arbitrum::usdc_e(),
                    decimals: 6,
                    index:    0,
                },
                TokenConfig {
                    symbol:   "USDT".into(),
                    address:  venues::arbitrum::usdt(),
                    decimals: 6,
                    index:    1,
                },
            ],
            is_meta: false,
            priority: 1,
        },
    ]
}

fn testnet_pools() -> Vec<PoolConfig> {
    // Fill with confirmed Sepolia addresses before running testnet integration tests.
    // Check https://curve.fi/#/arbitrum-sepolia/pools for current deployments.
    vec![]
}

// ─── BotParams ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotParams {
    /// Absolute minimum profit floor in USD. Any trade below this is skipped regardless of ROI.
    /// This is a safety net — the effective floor is max(min_profit_usd, gas_cost * min_gas_roi).
    pub min_profit_usd: f64,       // Default: 10.0

    /// Minimum ROI multiple on gas cost. Trade must profit at least this × gas cost.
    /// 3.0 = 300% ROI on gas. Prevents accepting $76 profit on $70 gas (8% ROI on gas).
    /// Combined with min_profit_usd: `effective_floor = max(min_profit_usd, gas_usd × min_gas_roi)`.
    /// Serde default: 3.0 — backward-compatible with older params.json files.
    #[serde(default = "default_min_gas_roi")]
    pub min_gas_roi: f64,           // Default: 3.0

    /// Pool must be this % off center to pass Layer 1 filter.
    pub min_imbalance_pct: f64,    // Default: 5.0

    /// Minimum velocity (imbalance change per block) for freshness filter.
    pub min_velocity: f64,         // Default: 0.015

    /// Bot halts when wallet ETH drops below this.
    pub gas_reserve_eth: f64,      // Default: 0.10

    /// Telegram alert fires when wallet drops below this.
    pub alert_gas_eth: f64,        // Default: 0.30

    /// Emergency borrow ceiling. Sizing is automatic; this is a last-resort guard.
    pub abs_cap_usd: f64,          // Default: 5_000_000.0 ($5M — raise empirically after measuring live P&L)

    /// Gas limit for arb transactions (deprecated global fallback).
    #[serde(default = "default_gas_limit_override")]
    pub gas_limit_override: u64,   // Default: 750_000

    /// Minimum simulated profit to justify routing via Timeboost express lane.
    #[serde(default = "default_timeboost_min_profit_usd")]
    pub timeboost_min_profit_usd: f64, // Default: 75.0

    /// Race-loss rate threshold (0.0 to 1.0) to trigger Timeboost bidding.
    #[serde(default = "default_timeboost_race_loss_threshold")]
    pub timeboost_race_loss_threshold: f64, // Default: 0.25 (25%)

    /// Stress priority fee multiplier applied to base_fee (default: 0.25 = 25%).
    #[serde(default = "default_stress_priority_fee_multiplier")]
    pub stress_priority_fee_multiplier: f64, // Default: 0.25

    /// Gas limit for 2-hop routes.
    #[serde(default = "default_gas_limit_2hop")]
    pub gas_limit_2hop: u64,       // Default: 350_000

    /// Gas limit for 3/4-hop routes.
    #[serde(default = "default_gas_limit_4hop")]
    pub gas_limit_4hop: u64,       // Default: 750_000

    /// Kill-switch for in-memory calldata cache.
    #[serde(default = "default_calldata_cache_enabled")]
    pub calldata_cache_enabled: bool, // Default: true

    /// Kill-switch for pre-signed transaction pool.
    #[serde(default = "default_presigned_pool_enabled")]
    pub presigned_pool_enabled: bool, // Default: true

    /// Preferred flash loan source (Balancer 0% vs Aave).
    #[serde(default = "default_flash_source_preference")]
    pub flash_source_preference: FlashSourcePreference,

    /// Tunable dynamic slippage model parameters.
    #[serde(default = "default_slippage_model")]
    pub slippage_model: SlippageModelParams,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum FlashSourcePreference {
    AaveOnly,
    #[default]
    BalancerPreferred,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SlippageModelParams {
    pub depth_base: f64,        // default 0.003
    pub depth_shallow: f64,     // default 0.012
    pub time_drift_rate: f64,   // default 0.0002
    pub time_drift_cap: f64,    // default 0.005
    pub size_ratio_weight: f64, // default 0.02
    pub hard_cap: f64,          // default 0.03
}

impl Default for SlippageModelParams {
    fn default() -> Self {
        Self {
            depth_base: 0.003,
            depth_shallow: 0.012,
            time_drift_rate: 0.0002,
            time_drift_cap: 0.005,
            size_ratio_weight: 0.02,
            hard_cap: 0.03,
        }
    }
}

impl Default for BotParams {
    fn default() -> Self {
        Self {
            min_profit_usd:                 10.0,   // absolute floor; effective floor = max(10, gas×3)
            min_gas_roi:                    3.0,    // 300% ROI on gas minimum
            min_imbalance_pct:              5.0,
            min_velocity:                   0.015,
            gas_reserve_eth:                0.10,
            alert_gas_eth:                  0.30,
            abs_cap_usd:                    5_000_000.0,
            gas_limit_override:             750_000,
            timeboost_min_profit_usd:       75.0,
            timeboost_race_loss_threshold:  0.25,
            stress_priority_fee_multiplier: 0.25,
            gas_limit_2hop:                 350_000,
            gas_limit_4hop:                 750_000,
            calldata_cache_enabled:         true,
            presigned_pool_enabled:         true,
            flash_source_preference:        FlashSourcePreference::BalancerPreferred,
            slippage_model:                 SlippageModelParams::default(),
        }
    }
}

/// Serde field defaults for backward-compatibility with older params.json.
fn default_min_gas_roi() -> f64 { 3.0 }
fn default_gas_limit_override() -> u64 { 750_000 }
fn default_timeboost_min_profit_usd() -> f64 { 75.0 }
fn default_timeboost_race_loss_threshold() -> f64 { 0.25 }
fn default_stress_priority_fee_multiplier() -> f64 { 0.25 }
fn default_gas_limit_2hop() -> u64 { 350_000 }
fn default_gas_limit_4hop() -> u64 { 750_000 }
fn default_calldata_cache_enabled() -> bool { true }
fn default_presigned_pool_enabled() -> bool { true }
fn default_flash_source_preference() -> FlashSourcePreference { FlashSourcePreference::BalancerPreferred }
fn default_slippage_model() -> SlippageModelParams { SlippageModelParams::default() }

impl BotParams {
    /// Effective profit floor for a given gas cost.
    ///
    /// `max(min_profit_usd, gas_cost × min_gas_roi)` — ensures the trade
    /// has acceptable ROI relative to gas spend, not just an absolute dollar floor.
    pub fn effective_min_profit_usd(&self, gas_cost_usd: f64) -> f64 {
        self.min_profit_usd.max(gas_cost_usd * self.min_gas_roi)
    }

    /// Select gas limit based on route hop count.
    pub fn gas_limit_for_route(&self, hops_count: usize) -> u64 {
        if hops_count <= 2 {
            self.gas_limit_2hop
        } else {
            self.gas_limit_4hop
        }
    }

    pub fn from_env() -> Self {
        let mut p = Self::default();

        // Load persisted parameter overrides from {KINGFISHER_DATA_DIR}/params.json if
        // present; falls through to env-var loading on first boot. The data dir defaults
        // to /var/lib/kingfisher on bare metal (see deploy/kingfisher.service).
        let params_path = format!("{}/params.json", data_dir());
        if let Ok(json) = std::fs::read_to_string(&params_path) {
            if let Ok(persisted) = serde_json::from_str::<Self>(&json) {
                tracing::info!(path = %params_path, "Loaded persisted params");
                p = persisted;
                return p; // persisted JSON takes priority over env vars
            } else {
                tracing::warn!(path = %params_path, "Found params.json but failed to parse — falling back to env vars");
            }
        }

        macro_rules! load {
            ($field:ident, $key:literal) => {
                if let Ok(v) = std::env::var($key) {
                    if let Ok(parsed) = v.parse() {
                        p.$field = parsed;
                    }
                }
            };
        }
        load!(min_profit_usd,                 "MIN_PROFIT_USD");
        load!(min_gas_roi,                    "MIN_GAS_ROI");
        load!(min_imbalance_pct,              "MIN_IMBALANCE_PCT");
        load!(min_velocity,                   "MIN_VELOCITY");
        load!(gas_reserve_eth,                "GAS_RESERVE_ETH");
        load!(alert_gas_eth,                  "ALERT_GAS_ETH");
        load!(abs_cap_usd,                    "ABS_CAP_USD");
        load!(gas_limit_override,             "GAS_LIMIT_OVERRIDE");
        load!(timeboost_min_profit_usd,       "TIMEBOOST_MIN_PROFIT_USD");
        load!(timeboost_race_loss_threshold,  "TIMEBOOST_RACE_LOSS_THRESHOLD");
        load!(stress_priority_fee_multiplier, "STRESS_PRIORITY_FEE_MULTIPLIER");
        load!(gas_limit_2hop,                 "GAS_LIMIT_2HOP");
        load!(gas_limit_4hop,                 "GAS_LIMIT_4HOP");
        load!(calldata_cache_enabled,         "CALLDATA_CACHE_ENABLED");
        load!(presigned_pool_enabled,         "PRESIGNED_POOL_ENABLED");

        if let Ok(v) = std::env::var("FLASH_SOURCE_PREFERENCE") {
            if v.eq_ignore_ascii_case("aave_only") {
                p.flash_source_preference = FlashSourcePreference::AaveOnly;
            } else if v.eq_ignore_ascii_case("balancer_preferred") {
                p.flash_source_preference = FlashSourcePreference::BalancerPreferred;
            }
        }

        p
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bot_params_defaults() {
        let p = BotParams::default();
        assert_eq!(p.min_profit_usd,    10.0);
        assert_eq!(p.min_gas_roi,        3.0);
        assert_eq!(p.min_imbalance_pct,  5.0);
        assert_eq!(p.min_velocity,       0.015);
        assert_eq!(p.gas_reserve_eth,    0.10);
        assert_eq!(p.alert_gas_eth,      0.30);
        assert_eq!(p.abs_cap_usd,        5_000_000.0);
    }

    #[test]
    fn test_effective_min_profit_usd() {
        let p = BotParams::default(); // min_profit_usd=10.0, min_gas_roi=3.0
        // Below absolute floor: gas=$1 → floor=max(10, 3)=$10
        assert_eq!(p.effective_min_profit_usd(1.0), 10.0);
        // Above absolute floor: gas=$10 → floor=max(10, 30)=$30
        assert_eq!(p.effective_min_profit_usd(10.0), 30.0);
        // Large gas: gas=$50 → floor=max(10, 150)=$150
        assert_eq!(p.effective_min_profit_usd(50.0), 150.0);
    }

    #[test]
    fn test_bot_params_from_env_overrides() {
        std::env::set_var("MIN_PROFIT_USD", "150");
        std::env::set_var("GAS_RESERVE_ETH", "0.20");
        let p = BotParams::from_env();
        assert_eq!(p.min_profit_usd,  150.0);
        assert_eq!(p.gas_reserve_eth,   0.20);
        // Clean up
        std::env::remove_var("MIN_PROFIT_USD");
        std::env::remove_var("GAS_RESERVE_ETH");
    }

    #[test]
    fn test_network_chain_ids() {
        assert_eq!(Network::Testnet.chain_id(), 421_614);
        assert_eq!(Network::Mainnet.chain_id(), 42_161);
        assert_eq!(Network::Monad.chain_id(), 143);
    }

    #[test]
    fn test_network_from_env_defaults_to_testnet() {
        std::env::remove_var("NETWORK");
        let n = Network::from_env();
        assert_eq!(n, Network::Testnet);
    }
}

/// Startup audit: surface any pool token / Aave USDC address mismatch.
/// Non-blocking — logs warnings so the operator can verify before the first trade.
pub async fn verify_token_alignment_log(network: &Network) {
    let aave_token = network.usdc_address(); // native USDC

    tracing::info!(
        aave_usdc   = %aave_token,
        "Token alignment check: Aave native USDC vs pool configs"
    );

    for pool in network.pools() {
        for token in &pool.tokens {
            if token.symbol.contains("USDC") && token.address != aave_token {
                tracing::warn!(
                    pool        = %pool.name,
                    pool_token  = %token.address,
                    aave_token  = %aave_token,
                    symbol      = %token.symbol,
                    "⚠️  Token mismatch: pool uses different USDC variant than Aave flash loan. \
                     Verify this route works before going live."
                );
            }
        }
    }

    // USDC.e address on Arbitrum One — known deprecated token
    let usdce: Address = "0xFF970A61A04b1cA14834A43f5dE4533eBDDB5CC8".parse().unwrap();

    for pool in network.pools() {
        for token in &pool.tokens {
            if token.address == usdce {
                tracing::warn!(
                    pool   = %pool.name,
                    "⚠️  Pool uses USDC.e (0xFF970...) — the deprecated bridged token. \
                     Native USDC (0xaf88d...) is what Aave V3 lends. \
                     Confirm exchange compatibility or update pool config."
                );
            }
        }
    }

    tracing::info!("Token alignment check complete");
}
