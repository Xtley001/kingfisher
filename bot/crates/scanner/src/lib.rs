#![allow(clippy::too_many_arguments)]
pub mod filters;
pub mod route_graph;

use anyhow::Result;
use rayon::prelude::*;
use kingfisher_core::{
    config::{BotParams, Network},
    types::{Opportunity, OpportunityEvent, PoolState},
};

/// Called once per block.
/// Returns the highest simulated-net-profit opportunity, or None.
pub fn scan_block(
    pool_states:  &[PoolState],
    base_fee:     u128,
    eth_price:    f64,
    aave_max:     u128,
    params:       &BotParams,
    stress:       bool,
    block:        u64,
    network:      &Network,
    aave_fee_bps: u64,
) -> Result<Option<Opportunity>> {

    // Layer 1+2: imbalance + velocity (parallel)
    let candidates: Vec<&PoolState> = pool_states
        .par_iter()
        .filter(|p| p.is_healthy())
        .filter(|p| filters::passes_imbalance(p, params))
        .filter(|p| filters::passes_velocity(p, params))
        .collect();

    if candidates.is_empty() {
        return Ok(None);
    }

    tracing::debug!(block = block, layer12_pass = candidates.len(), "Imbalance+velocity");

    // Build route graph and find all cycles through candidate pools
    let graph  = route_graph::RouteGraph::build(pool_states);
    let routes: Vec<Vec<kingfisher_core::types::RouteHop>> = candidates
        .par_iter()
        .flat_map(|pool| graph.find_cycles_from_pool(pool.address, 4))
        .collect();

    if routes.is_empty() {
        return Ok(None);
    }

    // Layer 3: estimated profit filter with dynamic gas-scaled floor.
    let profitable: Vec<Opportunity> = routes
        .into_iter()
        .filter_map(|route| {
            filters::estimate_profit(&route, pool_states, aave_max, eth_price, params, block, base_fee, aave_fee_bps)
        })
        // Secondary floor guard using stored gas cost.
        .filter(|opp| {
            let gas = opp.gas_cost_usd.unwrap_or(0.0);
            opp.estimated_profit_usd >= params.effective_min_profit_usd(gas)
        })
        .collect();

    if profitable.is_empty() {
        return Ok(None);
    }

    tracing::debug!(block = block, layer3_pass = profitable.len(), "Estimated profit filter");

    // Layer 5: algebraic net-profit simulation with live gas + Aave fee
    // (parallel across all candidates). The eth_call validator (validation.rs)
    // periodically cross-checks this against the live contract.
    let simulated: Vec<Opportunity> = profitable
        .into_par_iter()
        .filter_map(|mut opp| {
            let result = kingfisher_simulation::simulate_opportunity(
                &opp, network, base_fee, eth_price, aave_fee_bps,
            ).ok()?;

            // Dynamic floor based on simulated gas cost.
            if result.net_profit_usd < params.effective_min_profit_usd(result.gas_cost_usd) {
                return None;
            }

            opp.simulated_profit_usd = Some(result.net_profit_usd);
            opp.aave_fee_usd         = Some(result.aave_fee_usd);
            opp.gas_cost_usd         = Some(result.gas_cost_usd);
            Some(opp)
        })
        .collect();

    // Pick highest simulated profit
    let best = simulated
        .into_iter()
        .max_by(|a, b| {
            a.simulated_profit_usd
                .unwrap_or(0.0)
                .partial_cmp(&b.simulated_profit_usd.unwrap_or(0.0))
                .unwrap_or(std::cmp::Ordering::Equal)
        });

    if let Some(ref opp) = best {
        tracing::info!(
            block      = block,
            route      = %opp.route_description,
            profit_usd = opp.simulated_profit_usd,
            flash_usd  = opp.flash_amount as f64 / 1e6,
            stress     = stress,
            "🎯 Best opportunity"
        );
    }

    Ok(best)
}

/// Convert an Opportunity to a dashboard feed event
pub fn to_event(opp: &Opportunity, fired: bool) -> OpportunityEvent {
    OpportunityEvent {
        id:                    opp.id.clone(),
        route_description:     opp.route_description.clone(),
        gross_swap_profit_usd: opp.gross_swap_profit_usd,
        estimated_profit_usd:  opp.estimated_profit_usd,
        simulated_profit_usd:  opp.simulated_profit_usd,
        edge_trigger:          opp.edge_trigger.clone(),
        detected_at:           opp.detected_at,
        fired,
    }
}
