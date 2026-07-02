use alloy::providers::Provider;
use alloy::primitives::{Address, U256, Bytes};
use alloy::rpc::types::TransactionRequest;
use anyhow::Result;
use std::sync::Arc;

use kingfisher_core::{AaveReserveStatus, PoolState, config::{Network, PoolConfig}};
use crate::ChainState;

pub async fn fetch_all_state<P: Provider + Clone + Send + Sync + 'static>(
    provider: &Arc<P>,
    network:  &Network,
    pools:    &[PoolConfig],
    block:    u64,
) -> Result<ChainState> {
    let (eth_price, usdc_peg, usdt_peg, wallet_eth, aave_status, pool_states) = tokio::join!(
        call_chainlink(provider, network.chainlink_eth_usd()),
        call_chainlink(provider, network.chainlink_usdc_usd()),
        call_chainlink(provider, network.chainlink_usdt_usd()),
        fetch_wallet_eth(provider, network),
        fetch_aave_status(provider, network, block),
        fetch_pool_states(provider, pools, block),
    );

    // eth_price is critical: a zero price collapses gas cost to $0 and disables the
    // dynamic profit floor. Hard-fail so the block is skipped rather than trading blind.
    let eth_price_usd = eth_price
        .map_err(|e| anyhow::anyhow!("ETH/USD Chainlink feed failed — skipping block {}: {}", block, e))?;
    if eth_price_usd <= 0.0 {
        return Err(anyhow::anyhow!("ETH/USD Chainlink returned non-positive price ({}) — skipping block {}", eth_price_usd, block));
    }

    // Peg feeds: use 1.0 on failure (neutral/no-stress). Log a warning so the operator
    // knows the feed is down. The stress-regime detection may miss a real depeg during
    // an outage, but this is safer than halting the bot entirely.
    let usdc_peg_val = match usdc_peg {
        Ok(v) if v > 0.0 => v,
        Ok(_) | Err(_)   => {
            tracing::warn!(block, "USDC/USD Chainlink failed — using 1.0 (neutral peg) for this block");
            1.0
        }
    };
    let usdt_peg_val = match usdt_peg {
        Ok(v) if v > 0.0 => v,
        Ok(_) | Err(_)   => {
            tracing::warn!(block, "USDT/USD Chainlink failed — using 1.0 (neutral peg) for this block");
            1.0
        }
    };

    Ok(ChainState {
        eth_price_usd,
        usdc_peg:           usdc_peg_val,
        usdt_peg:           usdt_peg_val,
        wallet_eth_balance: wallet_eth.unwrap_or(0.0),
        aave_status:        aave_status.unwrap_or_default(),
        pool_states:        pool_states.unwrap_or_default(),
    })
}

/// Single eth_call via alloy 0.9 TransactionRequest
async fn raw_call<P: Provider + Clone>(
    provider: &Arc<P>,
    to:       Address,
    data:     Vec<u8>,
) -> Result<Bytes> {
    let req = TransactionRequest {
        to:    Some(alloy::primitives::TxKind::Call(to)),
        input: alloy::rpc::types::TransactionInput::new(Bytes::from(data)),
        ..Default::default()
    };
    Ok(provider.call(req).await?)
}

/// Read a Chainlink latestRoundData() price feed → USD price as f64.
/// Returns 0.0 if the price is stale (>4 hours) or out of range.
/// staleness guard added — previously updatedAt was decoded but never checked.
async fn call_chainlink<P: Provider + Clone>(
    provider: &Arc<P>,
    feed:     Address,
) -> Result<f64> {
    // latestRoundData() selector: 0xfeaf968c
    // ABI layout (each slot is 32 bytes):
    //   [0]  uint80  roundId
    //   [1]  int256  answer        → bytes [32..64]
    //   [2]  uint256 startedAt
    //   [3]  uint256 updatedAt     → bytes [96..128]
    //   [4]  uint80  answeredInRound
    let result = raw_call(provider, feed, hex::decode("feaf968c").unwrap()).await?;
    if result.len() < 128 {
        return Ok(0.0);
    }

    // ETH/USD Chainlink feed on Arbitrum One has a 1-hour heartbeat (3,600s),
    // not 2-hour. Using 4 hours meant the bot would trade for up to 3 hours after the
    // oracle went stale. 1 hour matches the actual ETH/USD feed heartbeat specification.
    // USDC/USD and USDT/USD are deviation-triggered (0.25%) — tolerate up to 24h for those,
    // but 1h is still safe as a blanket threshold given ETH/USD is the critical gas input.
    let mut ts_arr = [0u8; 16];
    ts_arr.copy_from_slice(&result[112..128]); // lower 16 bytes of updatedAt slot
    let updated_at = u128::from_be_bytes(ts_arr) as i64;
    let now = chrono::Utc::now().timestamp();
    const STALENESS_THRESHOLD_SECS: i64 = 3600;  // 1 hour — matches ETH/USD Arbitrum heartbeat
    if now - updated_at > STALENESS_THRESHOLD_SECS {
        tracing::warn!(
            feed     = %feed,
            age_secs = now - updated_at,
            threshold = STALENESS_THRESHOLD_SECS,
            "Chainlink price stale — treating as unavailable"
        );
        return Ok(0.0);
    }

    // Decode int256 answer at slot [1] (bytes 32..64), 8 decimals
    // previous code copied only the lower 16 bytes then attempted two's complement
    // using u128::MAX — incorrect for a 256-bit signed integer. In practice Chainlink never
    // returns negative prices, but a dormant bug during oracle malfunction could have produced
    // a wrapped-positive garbage value that passed the sanity check.
    // Fix: read the sign bit from the full 32-byte slot and decode magnitude correctly.
    let raw = &result[32..64];
    let is_neg = raw[0] & 0x80 != 0;
    // For Chainlink price feeds the answer always fits in i64 (it's a USD price * 1e8).
    // Read the lower 8 bytes which is sufficient for any sane price.
    let mut arr = [0u8; 8];
    arr.copy_from_slice(&raw[24..32]);
    let magnitude = u64::from_be_bytes(arr) as i64;
    let answer = if is_neg { -magnitude } else { magnitude };
    let price  = answer as f64 / 1e8;
    // tighten sanity bounds — USD price feeds should be in [$0.01, $100_000].
    // Old bounds [0, 1_000_000] were too wide and would accept garbage wrapped values.
    if price > 0.01 && price < 100_000.0 {
        return Ok(price);
    }
    Ok(0.0)
}
async fn fetch_wallet_eth<P: Provider + Clone>(
    provider: &Arc<P>,
    _network:  &Network,
) -> Result<f64> {
    let key = std::env::var("BOT_PRIVATE_KEY").unwrap_or_default();
    if key.is_empty() { return Ok(0.0); }
    use alloy::signers::local::PrivateKeySigner;
    let addr = key.parse::<PrivateKeySigner>()
        .map(|s| s.address())
        .unwrap_or(Address::ZERO);
    if addr == Address::ZERO { return Ok(0.0); }
    let bal = provider.get_balance(addr).await?;
    Ok(bal.to::<u128>() as f64 / 1e18)
}

async fn fetch_aave_status<P: Provider + Clone>(
    provider: &Arc<P>,
    network:  &Network,
    block:    u64,
) -> Result<AaveReserveStatus> {
    let pool = network.aave_pool();
    let usdc = network.usdc_address();
    // getReserveData(address) selector: 0x35ea6a75
    // Returns ReserveData struct; slot 0 is the 256-bit ReserveConfigurationMap bitmap.
    let mut data = hex::decode("35ea6a75").unwrap();
    data.extend_from_slice(&[0u8; 12]);
    data.extend_from_slice(usdc.as_slice());
    let result = raw_call(provider, pool, data).await.unwrap_or_default();

    // "not all zeros" is NOT a proxy for active.
    // ReserveConfigurationMap bitmap (Aave V3, slot 0 of getReserveData):
    //   bit 0:     isActive
    //   bit 1:     isFrozen      — flash loans disabled if frozen
    //   bit 57:    isPaused      — flash loans disabled if paused
    //   bits 80-115: borrowCap (in token units, 0 = uncapped)
    //
    // A flash loan requires: isActive=1 AND isFrozen=0 AND isPaused=0.
    let flash_enabled = if result.len() >= 32 {
        // Read the full 32-byte configuration bitmap (slot 0)
        let mut cfg_bytes = [0u8; 32];
        cfg_bytes.copy_from_slice(&result[0..32]);
        // Convert to u256 via u128 pairs for bit extraction
        let cfg_lo = u128::from_be_bytes(cfg_bytes[16..32].try_into().unwrap());
        let is_active = (cfg_lo & 0x1) != 0;
        let is_frozen = (cfg_lo & 0x2) != 0;
        let is_paused = (cfg_lo >> 57) & 0x1 != 0;
        is_active && !is_frozen && !is_paused
    } else {
        false
    };

    // decode actual borrow cap from bits 80-115 of the bitmap (in token units).
    // 0 means uncapped; we treat uncapped as a very large number (u128::MAX / 2).
    let borrow_cap = if result.len() >= 32 {
        let mut cfg_bytes = [0u8; 32];
        cfg_bytes.copy_from_slice(&result[0..32]);
        let cfg_lo = u128::from_be_bytes(cfg_bytes[16..32].try_into().unwrap());
        // bits 80-115 = 36-bit field for borrow cap
        let raw_cap = (cfg_lo >> 80) & ((1u128 << 36) - 1);
        if raw_cap == 0 {
            u128::MAX / 2   // 0 means no cap in Aave V3 — use a large sentinel
        } else {
            raw_cap * 1_000_000  // cap is in whole token units (USDC has 6 decimals)
        }
    } else {
        u128::MAX / 2
    };

    let liquidity = if result.len() >= 64 {
        let mut arr = [0u8; 16];
        arr.copy_from_slice(&result[48..64]);
        u128::from_be_bytes(arr)
    } else {
        0
    };

    if !flash_enabled {
        tracing::warn!(
            usdc = %usdc,
            "Aave reserve not flash-borrowable (inactive, frozen, or paused) — bot will skip arbs"
        );
    }

    // Read FLASHLOAN_PREMIUM_TOTAL() — selector 0x00a4b849
    // Returns uint128 (basis points). Governance-controlled; refresh every 1000 blocks.
    // This is the authoritative source for the flash loan fee — never hardcode it.
    let fee_bps = {
        let fee_data = hex::decode("00a4b849").unwrap();
        match raw_call(provider, pool, fee_data).await {
            Ok(r) if r.len() >= 32 => {
                let raw = u128::from_be_bytes(r[16..32].try_into().unwrap_or([0u8; 16]));
                if raw == 0 || raw > 1000 {
                    // Sanity check: fee must be 1–1000 bps (0.01%–10%)
                    tracing::warn!(raw, "FLASHLOAN_PREMIUM_TOTAL out of expected range — using 5 bps default");
                    5u64
                } else {
                    raw as u64
                }
            }
            Ok(_) => { tracing::warn!("FLASHLOAN_PREMIUM_TOTAL returned short data — using 5 bps"); 5 }
            Err(e) => { tracing::warn!(error = ?e, "FLASHLOAN_PREMIUM_TOTAL call failed — using 5 bps"); 5 }
        }
    };

    Ok(AaveReserveStatus {
        available_liquidity:  liquidity,
        borrow_cap,
        reserve_active:       flash_enabled,
        last_updated_block:   block,
        fee_bps,
        last_fee_read_block:  block,
    })
}

async fn fetch_pool_states<P: Provider + Clone + Send + Sync + 'static>(
    provider: &Arc<P>,
    pools:    &[PoolConfig],
    block:    u64,
) -> Result<Vec<PoolState>> {
    let mut handles = vec![];
    for cfg in pools {
        let p   = Arc::clone(provider);
        let cfg = cfg.clone();
        handles.push(tokio::spawn(async move {
            fetch_one_pool(&p, &cfg, block).await
        }));
    }
    let mut out = vec![];
    for h in handles {
        if let Ok(Ok(s)) = h.await { out.push(s); }
    }
    Ok(out)
}

async fn fetch_one_pool<P: Provider + Clone>(
    provider: &Arc<P>,
    cfg:      &PoolConfig,
    block:    u64,
) -> Result<PoolState> {
    let n = cfg.tokens.len();
    let mut raw  = vec![0u128; n];
    let mut norm = vec![0f64; n];

    for (i, token) in cfg.tokens.iter().enumerate() {
        // balances(uint256 i) selector: 0x4903b0d1
        let mut data = hex::decode("4903b0d1").unwrap();
        data.extend_from_slice(&U256::from(i).to_be_bytes::<32>());
        if let Ok(result) = raw_call(provider, cfg.address, data).await {
            if result.len() >= 32 {
                let mut arr = [0u8; 16];
                arr.copy_from_slice(&result[16..32]);
                raw[i]  = u128::from_be_bytes(arr);
                norm[i] = raw[i] as f64 / 10f64.powi(token.decimals as i32);
            }
        }
    }

    // A() selector: 0xf446c1d0
    let a: u64 = raw_call(provider, cfg.address, hex::decode("f446c1d0").unwrap())
        .await
        .ok()
        .filter(|r| r.len() >= 32)
        .map(|r| { let mut a = [0u8; 8]; a.copy_from_slice(&r[24..32]); u64::from_be_bytes(a) })
        .unwrap_or(500);

    // get_virtual_price() selector: 0xbb7b8b80
    let vp: u128 = raw_call(provider, cfg.address, hex::decode("bb7b8b80").unwrap())
        .await
        .ok()
        .filter(|r| r.len() >= 32)
        .map(|r| { let mut a = [0u8; 16]; a.copy_from_slice(&r[16..32]); u128::from_be_bytes(a) })
        .unwrap_or(1_000_000_000_000_000_000u128);

    // Read on-chain pool fee via fee() selector 0x90aaf60f (returns fee as 1e10 basis)
    let fee_rate: Option<f64> = raw_call(provider, cfg.address, hex::decode("90aaf60f").unwrap())
        .await
        .ok()
        .filter(|r| r.len() >= 32)
        .map(|r| {
            let mut a = [0u8; 4];
            a.copy_from_slice(&r[28..32]);
            let fee_raw = u32::from_be_bytes(a);
            fee_raw as f64 / 1e10 // Curve stores fee as integer / 1e10
        });

    let total: f64 = norm.iter().sum();
    Ok(PoolState {
        address:         cfg.address,
        name:            cfg.name.clone(),
        tokens:          cfg.tokens.clone(),
        balances_raw:    raw,
        balances_norm:   norm,
        total_norm:      total,
        a_parameter:     a,
        virtual_price:   vp,
        is_meta:         cfg.is_meta,
        balance_history: std::collections::VecDeque::new(),
        last_updated:    block,
        fee_rate,
    })
}
