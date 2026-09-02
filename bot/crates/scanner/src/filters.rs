use alloy::primitives::Address;
use kingfisher_core::{config::BotParams, types::{PoolState, Opportunity, RouteHop}};
use kingfisher_simulation::spread::StableSwapMath;
use kingfisher_simulation::sizing::{find_optimal_borrow_size, find_optimal_borrow_size_bidirectional};
use kingfisher_simulation::gas_usd_for_route;

pub fn passes_imbalance(pool: &PoolState, params: &BotParams) -> bool {
    pool.imbalance_ratio() > params.min_imbalance_pct / 100.0
}

pub fn passes_velocity(pool: &PoolState, params: &BotParams) -> bool {
    pool.velocity() > params.min_velocity
}

pub fn estimate_profit(
    route:        &[RouteHop],
    pool_states:  &[PoolState],
    aave_max:     u128,
    eth_price:    f64,
    params:       &BotParams,
    block:        u64,
    base_fee:     u128,
    aave_fee_bps: u64,
) -> Option<Opportunity> {
    if route.len() < 2 { return None; }

    let pool_a = pool_states.iter().find(|p| p.address == route[0].pool)?;
    let last   = route.last()?;
    let pool_b = pool_states.iter().find(|p| p.address == last.pool)?;

    let math_a = StableSwapMath::from_pool(pool_a);
    let math_b = StableSwapMath::from_pool(pool_b);

    let i = route[0].token_in_index as usize;
    let j = route[0].token_out_index as usize;

    let gas_est = gas_usd_for_route(route, base_fee, eth_price);

    let (borrow_fee_bps, flash_source) = match params.flash_source_preference {
        kingfisher_core::config::FlashSourcePreference::BalancerPreferred => (0.0, kingfisher_core::types::FlashSource::Balancer),
        kingfisher_core::config::FlashSourcePreference::AaveOnly => (aave_fee_bps as f64, kingfisher_core::types::FlashSource::Aave),
    };

    // Evaluate both directions on 2-hop routes; 3-hop routes use a fixed direction
    // (reversing intermediate hops without re-evaluating the full chain is incorrect).
    let (flash_amount, route_flipped) = if route.len() == 2 {
        find_optimal_borrow_size_bidirectional(
            &math_a, &math_b, i, j, borrow_fee_bps, aave_max, params.abs_cap_usd, gas_est,
        )
    } else {
        (find_optimal_borrow_size(
            &math_a, &math_b, i, j, borrow_fee_bps, aave_max, params.abs_cap_usd, gas_est,
        ), false)
    };
    if flash_amount == 0 { return None; }

    let flash_usd = flash_amount as f64 / 1e6;
    let flash_fee = flash_usd * (borrow_fee_bps / 10_000.0);

    // Use the effective pool direction chosen by the bidirectional search.
    let (eff_math_a, eff_math_b, eff_i, eff_j) = if route_flipped {
        (&math_b, &math_a, j, i)
    } else {
        (&math_a, &math_b, i, j)
    };

    let mid = eff_math_a.get_dy(eff_i, eff_j, flash_usd);
    if mid <= 0.0 { return None; }

    // For 3+ hop routes, chain get_dy() through every intermediate pool.
    let out = if route.len() == 2 {
        eff_math_b.get_dy(eff_j, eff_i, mid)
    } else {
        // Chain all intermediate hops using their pool's StableSwap math
        let mut amount = mid;
        for hop in route.iter().skip(1) {
            let pool = pool_states.iter().find(|p| p.address == hop.pool)?;
            let math  = StableSwapMath::from_pool(pool);
            amount = math.get_dy(hop.token_in_index as usize, hop.token_out_index as usize, amount);
            if amount <= 0.0 { return None; }
        }
        amount
    };

    let estimated_profit = out - flash_usd - flash_fee - gas_est;
    let effective_floor = params.effective_min_profit_usd(gas_est);
    if estimated_profit < effective_floor { return None; }

    let flash_token = if route_flipped {
        pool_b.tokens.get(eff_i).map(|t| t.address)?
    } else {
        pool_a.tokens.get(eff_i).map(|t| t.address)?
    };

    let mut effective_route = route.to_vec();
    if route_flipped {
        effective_route.reverse();
        for hop in &mut effective_route {
            std::mem::swap(&mut hop.token_in_index, &mut hop.token_out_index);
        }
        if let Some(h0) = effective_route.get_mut(0) {
            h0.token_in = pool_b.tokens.get(eff_i).map(|t| t.address).unwrap_or(flash_token);
        }
        if let Some(h1) = effective_route.get_mut(1) {
            h1.token_in = pool_a.tokens.get(eff_j).map(|t| t.address).unwrap_or(Address::ZERO);
        }
    }

    // Populate amount_in and expected_out for slippage protection
    if effective_route.len() == 2 {
        let dec_mid = if route_flipped {
            pool_b.tokens.get(eff_j).map(|t| t.decimals).unwrap_or(6)
        } else {
            pool_a.tokens.get(eff_j).map(|t| t.decimals).unwrap_or(6)
        };
        let dec_out = if route_flipped {
            pool_b.tokens.get(eff_i).map(|t| t.decimals).unwrap_or(6)
        } else {
            pool_a.tokens.get(eff_i).map(|t| t.decimals).unwrap_or(6)
        };

        effective_route[0].amount_in = flash_amount;
        effective_route[0].expected_out = (mid * 10f64.powi(dec_mid as i32)) as u128;
        effective_route[1].amount_in = effective_route[0].expected_out;
        effective_route[1].expected_out = (out * 10f64.powi(dec_out as i32)) as u128;
    }

    Some(Opportunity {
        id:                    uuid::Uuid::new_v4().to_string(),
        block_number:          block,
        detected_at:           chrono::Utc::now(),
        route_description:     describe(&effective_route, pool_states),
        route:                 effective_route,
        flash_token,
        flash_amount,
        gross_swap_profit_usd: out - flash_usd,
        estimated_profit_usd:  estimated_profit,
        simulated_profit_usd:  None,
        aave_fee_usd:          Some(flash_fee),
        gas_cost_usd:          Some(gas_est),
        edge_trigger:          None,
        flash_source,
    })
}

fn describe(route: &[RouteHop], pools: &[PoolState]) -> String {
    route.iter().filter_map(|hop| {
        let p   = pools.iter().find(|p| p.address == hop.pool)?;
        let tin  = p.tokens.get(hop.token_in_index  as usize).map(|t| t.symbol.as_str()).unwrap_or("?");
        let tout = p.tokens.get(hop.token_out_index as usize).map(|t| t.symbol.as_str()).unwrap_or("?");
        Some(format!("{}→{}({})", tin, tout, p.name))
    }).collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use alloy::primitives::Address;
    use kingfisher_core::config::{BotParams, TokenConfig};

    fn mock_pool(imbalance_pct: f64, velocity: f64) -> PoolState {
        // 10% imbalance on a 2-token pool means split is 55/45
        let b0 = 1_000_000.0 * (0.5 + imbalance_pct / 100.0);
        let b1 = 1_000_000.0 * (0.5 - imbalance_pct / 100.0);
        let total = b0 + b1;

        let mut history = VecDeque::new();
        // Add two history points to produce the requested velocity
        let vel_ratio_old = 0.5;
        let vel_ratio_new = 0.5 + velocity; // delta per block
        history.push_back((100u64, vec![vel_ratio_old * total, (1.0 - vel_ratio_old) * total]));
        history.push_back((101u64, vec![vel_ratio_new * total, (1.0 - vel_ratio_new) * total]));

        PoolState {
            address:         Address::ZERO,
            name:            "TEST".into(),
            tokens:          vec![
                TokenConfig { symbol: "A".into(), address: Address::ZERO, decimals: 18, index: 0 },
                TokenConfig { symbol: "B".into(), address: Address::ZERO, decimals: 18, index: 1 },
            ],
            balances_raw:    vec![b0 as u128, b1 as u128],
            balances_norm:   vec![b0, b1],
            total_norm:      total,
            a_parameter:     500,
            virtual_price:   1_000_000_000_000_000_000u128,
            is_meta:         false,
            balance_history: history,
            last_updated:    101,
            fee_rate:        Some(0.0004),
        }
    }

    #[test]
    fn test_imbalance_filter_pass() {
        let params = BotParams { min_imbalance_pct: 5.0, ..BotParams::default() };
        let pool   = mock_pool(10.0, 0.02); // 10% imbalance, easily passes
        assert!(passes_imbalance(&pool, &params), "10% should pass 5% threshold");
    }

    #[test]
    fn test_imbalance_filter_fail() {
        let params = BotParams { min_imbalance_pct: 5.0, ..BotParams::default() };
        let pool   = mock_pool(2.0, 0.02); // 2% imbalance — below threshold
        assert!(!passes_imbalance(&pool, &params), "2% should fail 5% threshold");
    }

    #[test]
    fn test_velocity_filter_pass() {
        let params = BotParams { min_velocity: 0.015, ..BotParams::default() };
        let pool   = mock_pool(10.0, 0.02); // velocity 0.02 > 0.015
        assert!(passes_velocity(&pool, &params), "velocity 0.02 should pass 0.015 threshold");
    }

    #[test]
    fn test_velocity_filter_fail() {
        let params = BotParams { min_velocity: 0.015, ..BotParams::default() };
        let pool   = mock_pool(10.0, 0.005); // velocity 0.005 < 0.015
        assert!(!passes_velocity(&pool, &params), "velocity 0.005 should fail 0.015 threshold");
    }
}
