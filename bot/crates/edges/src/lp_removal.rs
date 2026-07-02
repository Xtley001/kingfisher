//! Edge 9 — Large LP Removal Frontrunning
//! When a large LP removes liquidity from a Curve pool, the pool temporarily tilts.
//! Detect large remove_liquidity events and prepare the counter-trade.


/// remove_liquidity() selector: 0x5b36389c
/// remove_liquidity_one_coin() selector: 0x1a4d01d2
pub const REMOVE_LIQUIDITY_SELECTOR:          [u8; 4] = [0x5b, 0x36, 0x38, 0x9c];
pub const REMOVE_LIQUIDITY_ONE_COIN_SELECTOR: [u8; 4] = [0x1a, 0x4d, 0x01, 0xd2];

/// Minimum LP removal (USD) to qualify as a tradeable event
pub const MIN_REMOVAL_USD: f64 = 500_000.0;
