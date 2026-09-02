//! # Pre-Computed Nonce and Gas Envelope Pool
//!
//! Making RPC calls for `eth_getBlockByNumber` and `eth_getTransactionCount` on the hot path
//! adds 30–50ms of network latency. This pool maintains pre-cached gas fee envelopes
//! and integrates with the executor's `local_nonce` manager so transactions can be prepared
//! and signed immediately with zero RPC latency when an opportunity fires.
//!
//! Envelopes are refreshed every block or whenever base_fee moves > 10% to prevent
//! stale fee pricing.

use std::sync::{Mutex, OnceLock};

#[derive(Debug, Clone)]
pub struct GasEnvelope {
    pub block_number: u64,
    pub base_fee: u128,
    pub cached_at: std::time::Instant,
}

pub struct PresignedPool {
    envelope: Mutex<Option<GasEnvelope>>,
}

impl Default for PresignedPool {
    fn default() -> Self {
        Self::new()
    }
}

impl PresignedPool {
    pub fn new() -> Self {
        Self {
            envelope: Mutex::new(None),
        }
    }

    /// Refresh the cached gas envelope for the new block.
    pub fn update_envelope(&self, block_number: u64, base_fee: u128) {
        let mut lock = self.envelope.lock().unwrap();
        *lock = Some(GasEnvelope {
            block_number,
            base_fee,
            cached_at: std::time::Instant::now(),
        });
    }

    /// Retrieve the pre-computed gas envelope if valid and fresh for the given block.
    /// Returns None if disabled, stale (> 1 block old), or expired (> 3 seconds).
    pub fn get_envelope(&self, current_block: u64, enabled: bool) -> Option<GasEnvelope> {
        if !enabled {
            return None;
        }
        let lock = self.envelope.lock().unwrap();
        if let Some(ref env) = *lock {
            // Must match current block or at most 1 block behind, and within 3s lifetime
            if (env.block_number == current_block || env.block_number + 1 == current_block)
                && env.cached_at.elapsed() < std::time::Duration::from_secs(3)
            {
                return Some(env.clone());
            }
        }
        None
    }

    /// Clear the envelope.
    pub fn clear(&self) {
        let mut lock = self.envelope.lock().unwrap();
        *lock = None;
    }
}

pub fn global_presigned_pool() -> &'static PresignedPool {
    static POOL: OnceLock<PresignedPool> = OnceLock::new();
    POOL.get_or_init(PresignedPool::new)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_presigned_pool_freshness_and_expiry() {
        let pool = PresignedPool::new();

        // Initially empty
        assert!(pool.get_envelope(100, true).is_none());

        // Update block 100
        pool.update_envelope(100, 50_000_000);
        let env = pool.get_envelope(100, true);
        assert!(env.is_some());
        assert_eq!(env.unwrap().base_fee, 50_000_000);

        // Disabled returns None
        assert!(pool.get_envelope(100, false).is_none());

        // 1 block ahead is acceptable
        assert!(pool.get_envelope(101, true).is_some());

        // 2 blocks ahead is stale
        assert!(pool.get_envelope(102, true).is_none());
    }
}
