//! # Block-Scoped Calldata Cache
//!
//! Re-encoding `executeArb()` calldata takes time and allocs on the hot path.
//! Calldata is deterministic for a given route + pool state within the same block.
//! This cache stores encoded calldata keyed by `(block_number, route_hash)`.
//! Entries are strictly invalidated when the block advances to prevent stale slippage floors.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use alloy::primitives::Bytes;
use alloy::primitives::keccak256;
use kingfisher_core::types::Opportunity;

/// Global thread-safe calldata cache.
pub struct CalldataCache {
    entries: Mutex<HashMap<(u64, [u8; 32]), Bytes>>,
}

impl Default for CalldataCache {
    fn default() -> Self {
        Self::new()
    }
}

impl CalldataCache {
    pub fn new() -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
        }
    }

    /// Compute a fast 32-byte hash identifying the route hops and flash token.
    pub fn compute_route_hash(opp: &Opportunity) -> [u8; 32] {
        let mut data = Vec::new();
        data.extend_from_slice(opp.flash_token.as_slice());
        data.extend_from_slice(&opp.flash_amount.to_be_bytes());
        for hop in &opp.route {
            data.extend_from_slice(hop.pool.as_slice());
            data.extend_from_slice(hop.token_in.as_slice());
            data.extend_from_slice(&hop.token_in_index.to_be_bytes());
            data.extend_from_slice(&hop.token_out_index.to_be_bytes());
            data.push(if hop.is_meta { 1 } else { 0 });
            data.extend_from_slice(&hop.amount_in.to_be_bytes());
            data.extend_from_slice(&hop.expected_out.to_be_bytes());
        }
        *keccak256(&data)
    }

    /// Look up calldata for the given opportunity and block number.
    /// Returns `Some(Bytes)` on hit, or `None` on miss / disabled.
    pub fn get(&self, opp: &Opportunity, block: u64, enabled: bool) -> Option<Bytes> {
        if !enabled {
            return None;
        }
        let hash = Self::compute_route_hash(opp);
        let mut map = self.entries.lock().unwrap();

        // Invalidate older block entries to prevent memory growth and stale slippage
        map.retain(|&(b, _), _| b >= block);

        map.get(&(block, hash)).cloned()
    }

    /// Insert newly encoded calldata for the given opportunity and block number.
    pub fn insert(&self, opp: &Opportunity, block: u64, calldata: Bytes, enabled: bool) {
        if !enabled {
            return;
        }
        let hash = Self::compute_route_hash(opp);
        let mut map = self.entries.lock().unwrap();
        map.retain(|&(b, _), _| b >= block);
        map.insert((block, hash), calldata);
    }

    /// Explicitly clear the cache (e.g. on new block or reorg).
    pub fn clear(&self) {
        let mut map = self.entries.lock().unwrap();
        map.clear();
    }
}

pub fn global_calldata_cache() -> &'static CalldataCache {
    static CACHE: OnceLock<CalldataCache> = OnceLock::new();
    CACHE.get_or_init(CalldataCache::new)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use alloy::primitives::Address;
    use kingfisher_core::types::RouteHop;

    fn make_test_opp(flash_amt: u128) -> Opportunity {
        Opportunity {
            id: "test-cache-opp".into(),
            block_number: 100,
            detected_at: Utc::now(),
            route: vec![RouteHop {
                pool: Address::repeat_byte(0x01),
                pool_name: "PoolA".into(),
                token_in: Address::repeat_byte(0x02),
                token_in_index: 0,
                token_out_index: 1,
                is_meta: false,
                amount_in: 1000,
                expected_out: 999,
            }],
            route_description: "test".into(),
            flash_token: Address::repeat_byte(0x03),
            flash_amount: flash_amt,
            gross_swap_profit_usd: 10.0,
            estimated_profit_usd: 5.0,
            simulated_profit_usd: Some(5.0),
            aave_fee_usd: None,
            gas_cost_usd: None,
            edge_trigger: None,
            flash_source: kingfisher_core::types::FlashSource::Aave,
        }
    }

    #[test]
    fn test_calldata_cache_hit_and_miss() {
        let cache = CalldataCache::new();
        let opp = make_test_opp(1000);
        let bytes = Bytes::from_static(b"dummy_calldata");

        // Miss initially
        assert_eq!(cache.get(&opp, 100, true), None);

        // Insert and hit
        cache.insert(&opp, 100, bytes.clone(), true);
        assert_eq!(cache.get(&opp, 100, true), Some(bytes.clone()));

        // Disabled returns None
        assert_eq!(cache.get(&opp, 100, false), None);
    }

    #[test]
    fn test_calldata_cache_invalidates_on_block_advance() {
        let cache = CalldataCache::new();
        let opp = make_test_opp(1000);
        let bytes = Bytes::from_static(b"dummy_calldata");

        // Insert at block 100
        cache.insert(&opp, 100, bytes.clone(), true);
        assert_eq!(cache.get(&opp, 100, true), Some(bytes.clone()));

        // Advance to block 101: looking up block 101 misses, and purges block 100
        assert_eq!(cache.get(&opp, 101, true), None);

        // Verify block 100 entry is purged and cannot be read
        assert_eq!(cache.get(&opp, 100, true), None);
    }
}
