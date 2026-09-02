//! Pre-computed depeg opportunity templates — Edge 4 acceleration.
//! Built during calm markets every 100 blocks.
//! When peg stress fires, templates execute immediately — zero compute lag on the hot path.

use std::collections::HashMap;
use alloy::primitives::Address;

use kingfisher_core::{
    config::{BotParams, Network},
    types::{Opportunity, RouteHop, PoolState},
};
use kingfisher_simulation::{
    sizing::find_optimal_borrow_size_bidirectional,
    spread::StableSwapMath,
};

#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub enum DepegScenario {
    UsdcDown,
    UsdtDown,
    FraxDown,
}

#[derive(Debug, Clone)]
pub struct DepegTemplates {
    pub templates:      HashMap<DepegScenario, Opportunity>,
    pub built_at_block: u64,
}

impl DepegTemplates {
    pub fn build(
        pool_states:    &[PoolState],
        aave_max:       u128,
        params:         &BotParams,
        _network:       &Network,
        block:          u64,
        base_fee:       u128,   // live base fee for accurate gas estimate
        eth_price:      f64,    // live ETH/USD price for accurate gas estimate
        aave_fee_bps:   u64,    // runtime-read from FLASHLOAN_PREMIUM_TOTAL() — never hardcode
    ) -> Self {
        let mut templates = HashMap::new();

        // USDC Down: borrow USDT, buy cheap USDC on 2pool, return USDT via crvUSD-USDT
        if let Some(opp) = make_template(
            pool_states, aave_max, params, block, base_fee, eth_price, aave_fee_bps,
            "USDC Down",
            "0xaf88d065e77c8cC2239327C5EDb3A432268e5831", // native USDC (Aave-compatible)
            "0xFd086bC7CD5C481DCC9C85ebE478A1C0b69FCbb9", // flash token: USDT
        ) {
            templates.insert(DepegScenario::UsdcDown, opp);
        }

        // USDT Down: borrow USDC, buy cheap USDT
        if let Some(opp) = make_template(
            pool_states, aave_max, params, block, base_fee, eth_price, aave_fee_bps,
            "USDT Down",
            "0xFd086bC7CD5C481DCC9C85ebE478A1C0b69FCbb9", // cheap: USDT
            "0xaf88d065e77c8cC2239327C5EDb3A432268e5831", // native USDC (Aave-compatible)
        ) {
            templates.insert(DepegScenario::UsdtDown, opp);
        }

        // FRAX Down: borrow USDC, buy cheap FRAX on FRAX-USDC
        if let Some(opp) = make_template(
            pool_states, aave_max, params, block, base_fee, eth_price, aave_fee_bps,
            "FRAX Down",
            "0x17FC002b466eEc40DaE837Fc4bE5c67993ddBd6F", // cheap: FRAX
            "0xaf88d065e77c8cC2239327C5EDb3A432268e5831", // native USDC (Aave-compatible)
        ) {
            templates.insert(DepegScenario::FraxDown, opp);
        }

        let count = templates.len();
        tracing::debug!(block, templates = count, "Depeg templates rebuilt");
        Self { templates, built_at_block: block }
    }

    pub fn is_stale(&self, current_block: u64) -> bool {
        current_block.saturating_sub(self.built_at_block) > 100
    }

    pub fn get(&self, scenario: &DepegScenario) -> Option<&Opportunity> {
        self.templates.get(scenario)
    }

    /// Which scenario matches current peg prices? Returns the worst-deviation match.
    pub fn active_scenario(usdc: f64, usdt: f64, frax: f64) -> Option<DepegScenario> {
        let mut candidates: Vec<(f64, DepegScenario)> = vec![
            ((usdc - 1.0).abs(), DepegScenario::UsdcDown),
            ((usdt - 1.0).abs(), DepegScenario::UsdtDown),
            ((frax - 1.0).abs(), DepegScenario::FraxDown),
        ];
        candidates.retain(|(dev, _)| *dev > 0.002);
        candidates.sort_by(|(a, _), (b, _)| b.partial_cmp(a).unwrap());
        candidates.into_iter().next().map(|(_, s)| s)
    }
}

/// Build a 2-hop template: flash `flash_token` → buy `cheap_token` → sell back.
fn make_template(
    pool_states:  &[PoolState],
    aave_max:     u128,
    params:       &BotParams,
    block:        u64,
    base_fee:     u128,
    eth_price:    f64,
    aave_fee_bps: u64,
    label:        &str,
    cheap_token:  &str,
    flash_token:  &str,
) -> Option<Opportunity> {
    let cheap_addr: Address = cheap_token.parse().ok()?;
    let flash_addr: Address = flash_token.parse().ok()?;

    // Pool that holds both tokens (use it as the arb pool)
    let arb_pool = pool_states.iter().find(|p| {
        p.tokens.iter().any(|t| t.address == cheap_addr)
            && p.tokens.iter().any(|t| t.address == flash_addr)
    })?;

    let i_flash = arb_pool.tokens.iter().position(|t| t.address == flash_addr)?;
    let j_cheap = arb_pool.tokens.iter().position(|t| t.address == cheap_addr)?;

    let math = StableSwapMath::from_pool(arb_pool);

    // For a valid depeg trade we need a second pool or the same pool for the return leg.
    // If there's a second pool use it; otherwise use the same pool (less alpha but valid).
    let return_pool = pool_states.iter().find(|p| {
        p.address != arb_pool.address
            && p.tokens.iter().any(|t| t.address == cheap_addr)
            && p.tokens.iter().any(|t| t.address == flash_addr)
    });

    let (return_math, i_ret, j_ret, _ret_addr, ret_name) = if let Some(rp) = return_pool {
        let ir = rp.tokens.iter().position(|t| t.address == cheap_addr)?;
        let jr = rp.tokens.iter().position(|t| t.address == flash_addr)?;
        (StableSwapMath::from_pool(rp), ir, jr, rp.address, rp.name.clone())
    } else {
        // Reverse on same pool (will have lower profit — serves as warm cache)
        (math.clone(), j_cheap, i_flash, arb_pool.address, arb_pool.name.clone())
    };

    // Optimal sizing for the return leg — bidirectional, gas-aware (P4, P7 fixes)
    // Templates use a conservative 0.1 gwei / $3000 ETH gas estimate (overridden at L5 sim)
    // Live gas estimate: uses actual base_fee and eth_price threaded from the block loop.
    // Previously hardcoded 0.1 gwei / $3000 ETH — could be 7x off at peak fee / ETH price.
    let template_gas_est = {
        let units     = (150_000u64 + 80_000 * 2u64) as f64; // 2-hop
        let gwei      = base_fee as f64 / 1e9;
        let eth_spent = units * gwei / 1e9;
        eth_spent * eth_price
    };
    let (flash_amount, _route_flipped) = find_optimal_borrow_size_bidirectional(
        &return_math, &math, i_ret, j_ret, aave_fee_bps as f64, aave_max, params.abs_cap_usd, template_gas_est,
    );
    if flash_amount == 0 { return None; }

    let flash_usd = flash_amount as f64 / 1e6;
    let aave_fee  = flash_usd * (aave_fee_bps as f64 / 10_000.0);
    let mid       = return_math.get_dy(i_ret, j_ret, flash_usd);
    if mid <= 0.0 { return None; }
    let out       = math.get_dy(j_cheap, i_flash, mid);

    // Template gas estimate: conservative 0.1 gwei / $3000 ETH; overridden at L5 with live values.
    // template_gas_est was already computed above for the sizing call — reuse it here.
    let est_profit = out - flash_usd - aave_fee - template_gas_est;
    // use effective profit floor (dynamic gas ROI check)
    if est_profit < params.effective_min_profit_usd(template_gas_est) { return None; }

    let tok_flash = arb_pool.tokens.get(i_flash).map(|t| t.symbol.clone()).unwrap_or_default();
    let tok_cheap = arb_pool.tokens.get(j_cheap).map(|t| t.symbol.clone()).unwrap_or_default();
    let cheap_dec = arb_pool.tokens.get(j_cheap).map(|t| t.decimals).unwrap_or(18);
    let flash_dec = arb_pool.tokens.get(i_flash).map(|t| t.decimals).unwrap_or(6);

    let mid_wei = (mid * 10f64.powi(cheap_dec as i32)) as u128;
    let out_wei = (out * 10f64.powi(flash_dec as i32)) as u128;

    let route = vec![
        RouteHop {
            pool:            return_pool.map(|p| p.address).unwrap_or(arb_pool.address),
            pool_name:       ret_name.clone(),
            token_in:        flash_addr,
            token_in_index:  i_ret as i128,
            token_out_index: j_ret as i128,
            is_meta:         return_pool.map(|p| p.is_meta).unwrap_or(arb_pool.is_meta),
            amount_in:       flash_amount,
            expected_out:    mid_wei,
        },
        RouteHop {
            pool:            arb_pool.address,
            pool_name:       arb_pool.name.clone(),
            token_in:        cheap_addr,
            token_in_index:  j_cheap as i128,
            token_out_index: i_flash as i128,
            is_meta:         arb_pool.is_meta,
            amount_in:       mid_wei,
            expected_out:    out_wei,
        },
    ];

    Some(Opportunity {
        id:                    uuid::Uuid::new_v4().to_string(),
        block_number:          block,
        detected_at:           chrono::Utc::now(),
        route,
        route_description:     format!("{}: {}→{}({})→{}({})",
            label, tok_flash, tok_cheap, ret_name, tok_flash, arb_pool.name),
        flash_token:           flash_addr,
        flash_amount,
        gross_swap_profit_usd: out - flash_usd,
        estimated_profit_usd:  est_profit,
        simulated_profit_usd:  Some(est_profit * 0.95),
        aave_fee_usd:          Some(aave_fee),
        gas_cost_usd:          Some(template_gas_est),
        edge_trigger:          Some(label.into()),
        flash_source:          kingfisher_core::types::FlashSource::Aave,
    })
}
