//! # Route Graph
//!
//! Directed graph over token addresses where edges are pool swap rates.
//! Finds profitable arbitrage cycles using DFS (≤15 tokens) or Bellman-Ford
//! (>15 tokens). A negative-weight cycle in log-price space is a profitable arb.

use alloy::primitives::Address;
use std::collections::{HashMap, HashSet};
use kingfisher_core::types::{PoolState, RouteHop};

/// Directed graph: nodes = token addresses, edges = pool swap rates.
pub struct RouteGraph {
    adj:    HashMap<Address, Vec<EdgeData>>,
    tokens: Vec<Address>,    // canonical token ordering for Bellman-Ford
    edges:  Vec<BFEdge>,     // flat edge list for Bellman-Ford
}

struct EdgeData {
    to_token:  Address,
    pool:      Address,
    pool_name: String,
    i:         i128,
    j:         i128,
    is_meta:   bool,
    rate:      f64,
}

/// Flat edge used by Bellman-Ford (log-price weights).
#[derive(Clone)]
struct BFEdge {
    from:      Address,
    to:        Address,
    pool:      Address,
    pool_name: String,
    i:         i128,
    j:         i128,
    is_meta:   bool,
    weight:    f64,  // -ln(rate) — negative cycle = profitable arb
}

impl RouteGraph {
    pub fn build(pool_states: &[PoolState]) -> Self {
        let mut adj:    HashMap<Address, Vec<EdgeData>> = HashMap::new();
        let mut edges:  Vec<BFEdge>                    = Vec::new();
        let mut tokens: HashSet<Address> = HashSet::new();

        for pool in pool_states {
            if !pool.is_healthy() { continue; }
            let n = pool.tokens.len();
            for i in 0..n {
                for j in 0..n {
                    if i == j { continue; }
                    let rate = pool.exchange_rate(i, j);
                    if rate <= 0.0 { continue; }

                    let from_tok = pool.tokens[i].address;
                    let to_tok   = pool.tokens[j].address;
                    tokens.insert(from_tok);
                    tokens.insert(to_tok);

                    // DFS adjacency list
                    adj.entry(from_tok).or_default().push(EdgeData {
                        to_token:  to_tok,
                        pool:      pool.address,
                        pool_name: pool.name.clone(),
                        i:         i as i128,
                        j:         j as i128,
                        is_meta:   pool.is_meta,
                        rate,
                    });

                    // Bellman-Ford edge list: weight = -ln(rate)
                    // Aave 0.05% fee baked in: effective rate × 0.9995
                    let effective = rate * 0.9995;
                    edges.push(BFEdge {
                        from:      from_tok,
                        to:        to_tok,
                        pool:      pool.address,
                        pool_name: pool.name.clone(),
                        i:         i as i128,
                        j:         j as i128,
                        is_meta:   pool.is_meta,
                        weight:    -effective.ln(),
                    });
                }
            }
        }

        Self {
            adj,
            tokens: tokens.into_iter().collect(),
            edges,
        }
    }

    /// Find all profitable cycles through a specific pool.
    /// Selects DFS for ≤15 tokens or Bellman-Ford for larger graphs.
    pub fn find_cycles_from_pool(&self, pool_addr: Address, max_hops: usize) -> Vec<Vec<RouteHop>> {
        if self.tokens.len() > 15 {
            tracing::debug!(tokens = self.tokens.len(), "Using Bellman-Ford");
            self.find_arb_cycles_bf(pool_addr)
        } else {
            self.find_arb_cycles_dfs(pool_addr, max_hops)
        }
    }

    // ── DFS (small graphs) ───────────────────────────────────────────────────

    fn find_arb_cycles_dfs(&self, pool_addr: Address, max_hops: usize) -> Vec<Vec<RouteHop>> {
        let mut results = vec![];
        for start_token in self.adj.keys() {
            let mut path = vec![];
            self.dfs(*start_token, *start_token, &mut path, 1.0, max_hops, &mut results, Some(pool_addr));
        }
        results
    }

    fn dfs(
        &self,
        current:       Address,
        start:         Address,
        path:          &mut Vec<RouteHop>,
        rate_product:  f64,
        hops_left:     usize,
        results:       &mut Vec<Vec<RouteHop>>,
        required_pool: Option<Address>,
    ) {
        if hops_left == 0 { return; }
        let Some(edges) = self.adj.get(&current) else { return };

        for edge in edges {
            if path.iter().any(|h| h.pool == edge.pool) { continue; }
            let new_rate = rate_product * edge.rate;

            if edge.to_token == start && !path.is_empty() {
                let meets_req = required_pool
                    .map(|rp| path.iter().any(|h| h.pool == rp) || edge.pool == rp)
                    .unwrap_or(true);

                if new_rate > 1.0005 && meets_req {
                    let mut route = path.clone();
                    route.push(RouteHop {
                        pool:            edge.pool,
                        pool_name:       edge.pool_name.clone(),
                        token_in_index:  edge.i,
                        token_out_index: edge.j,
                        is_meta:         edge.is_meta,
                        amount_in:       0,
                        expected_out:    0,
                    });
                    results.push(route);
                }
                continue;
            }
            if edge.to_token == start { continue; }

            path.push(RouteHop {
                pool:            edge.pool,
                pool_name:       edge.pool_name.clone(),
                token_in_index:  edge.i,
                token_out_index: edge.j,
                is_meta:         edge.is_meta,
                amount_in:       0,
                expected_out:    0,
            });
            self.dfs(edge.to_token, start, path, new_rate, hops_left - 1, results, required_pool);
            path.pop();
        }
    }

    // ── Bellman-Ford (large graphs) ──────────────────────────────────────────

    /// Bellman-Ford on the log-price graph. Complexity O(V × E).
    /// A negative-weight cycle in −ln(rate) space is a profitable arb.
    fn find_arb_cycles_bf(&self, required_pool: Address) -> Vec<Vec<RouteHop>> {
        let n = self.tokens.len();
        if n == 0 || self.edges.is_empty() { return vec![]; }

        // Token → index mapping
        let tok_idx: HashMap<Address, usize> = self.tokens.iter()
            .enumerate()
            .map(|(i, t)| (*t, i))
            .collect();

        let inf = f64::INFINITY;
        let mut results = vec![];

        // Run Bellman-Ford from each token as source
        for src_tok in &self.tokens {
            let src = tok_idx[src_tok];

            // dist[v] = shortest log-price path from src to v
            let mut dist:  Vec<f64>               = vec![inf; n];
            let mut pred:  Vec<Option<usize>>      = vec![None; n];
            let mut pred_e: Vec<Option<&BFEdge>>   = vec![None; n];

            dist[src] = 0.0;

            // Relax V-1 times
            for _ in 0..n.saturating_sub(1) {
                for e in &self.edges {
                    let Some(&u) = tok_idx.get(&e.from) else { continue };
                    let Some(&v) = tok_idx.get(&e.to)   else { continue };
                    if dist[u] < inf && dist[u] + e.weight < dist[v] {
                        dist[v]   = dist[u] + e.weight;
                        pred[v]   = Some(u);
                        pred_e[v] = Some(e);
                    }
                }
            }

            // Detect negative cycles (V-th relaxation finds further improvement)
            for e in &self.edges {
                let Some(&u) = tok_idx.get(&e.from) else { continue };
                let Some(&v) = tok_idx.get(&e.to)   else { continue };
                if dist[u] < inf && dist[u] + e.weight < dist[v] {
                    // v is on or reachable from a negative cycle.
                    // Trace back to reconstruct the cycle.
                    if let Some(route) = self.trace_cycle(v, &pred, &pred_e, &tok_idx, required_pool) {
                        // Confirm the cycle actually touches the required pool
                        if route.iter().any(|h| h.pool == required_pool) {
                            results.push(route);
                        }
                    }
                }
            }
        }

        // Deduplicate (same edge set, different starting index)
        deduplicate_routes(results)
    }

    /// Walk predecessors to extract the cycle containing vertex `v`.
    fn trace_cycle(
        &self,
        mut v:      usize,
        pred:       &[Option<usize>],
        pred_e:     &[Option<&BFEdge>],
        _tok_idx:   &HashMap<Address, usize>,
        _req_pool:  Address,
    ) -> Option<Vec<RouteHop>> {
        // Walk back n steps to guarantee we're inside the cycle
        let n = pred.len();
        for _ in 0..n { v = pred[v]?; }

        // Now walk the cycle collecting edges
        let cycle_start = v;
        let mut hops    = vec![];
        let mut cur     = v;
        let mut iters   = 0;

        loop {
            iters += 1;
            if iters > n + 2 { break; } // safety

            let edge = pred_e[cur]?;
            hops.push(RouteHop {
                pool:            edge.pool,
                pool_name:       edge.pool_name.clone(),
                token_in_index:  edge.i,
                token_out_index: edge.j,
                is_meta:         edge.is_meta,
                amount_in:       0,
                expected_out:    0,
            });
            cur = pred[cur]?;
            if cur == cycle_start { break; }
        }

        if hops.len() < 2 { return None; }
        hops.reverse(); // forward order
        Some(hops)
    }
}

/// Remove duplicate cycles (same pool set, different traversal order).
fn deduplicate_routes(routes: Vec<Vec<RouteHop>>) -> Vec<Vec<RouteHop>> {
    let mut seen: HashSet<Vec<Address>> = HashSet::new();
    routes.into_iter()
        .filter(|route| {
            let mut key: Vec<Address> = route.iter().map(|h| h.pool).collect();
            key.sort();
            seen.insert(key)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_route_graph_builds_with_empty_pools() {
        let graph = RouteGraph::build(&[]);
        let cycles = graph.find_cycles_from_pool(Address::ZERO, 4);
        assert!(cycles.is_empty());
    }

    #[test]
    fn test_dedup_removes_same_pool_sets() {
        // Create two routes with same pools but different order
        let pool_a = Address::from([1u8; 20]);
        let pool_b = Address::from([2u8; 20]);

        fn hop(pool: Address) -> RouteHop {
            RouteHop { pool, pool_name: "".into(), token_in_index: 0,
                token_out_index: 1, is_meta: false, amount_in: 0, expected_out: 0 }
        }

        let routes = vec![
            vec![hop(pool_a), hop(pool_b)],
            vec![hop(pool_b), hop(pool_a)],
            vec![hop(pool_a), hop(pool_b)],
        ];
        let deduped = deduplicate_routes(routes);
        assert_eq!(deduped.len(), 1, "Same pool sets should be deduplicated");
    }
}
