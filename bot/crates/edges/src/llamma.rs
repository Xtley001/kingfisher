//! Edge 10 — LLAMMA Band Crossing Monitor
//! Curve's LLAMMA engine runs continuous soft liquidations on crvUSD collateral.
//! When ETH price crosses a band boundary, LLAMMA buys/sells ETH for crvUSD —
//! this predictably pushes crvUSD pools off peg.
//! Direction is deterministic from the oracle price movement — we can pre-stage.

use alloy::primitives::Address;

/// LLAMMA WETH controller on Arbitrum One
pub const LLAMMA_WETH_CONTROLLER: &str = "0x1E0165DbD2019441aB7927C018701f3138114D71";

/// crvUSD-USDC pool gets pushed when LLAMMA is liquidating (ETH down)
pub const CRVUSD_USDC_POOL: &str = "0xec090cf6DD891D2d014beA6edAda6e05E025D93d";
/// crvUSD-USDT pool gets pushed during LLAMMA minting (ETH up)
pub const CRVUSD_USDT_POOL: &str = "0x73aF1150F265419Ef8a5DB41908B700C32D49135";

#[derive(Debug, Clone)]
pub struct LlammaSignal {
    /// Which pool will be tilted
    pub pool:       Address,
    /// Expected tilt direction: true = crvUSD will become excess (sell crvUSD)
    pub crvusd_excess: bool,
    /// Estimated push strength based on band depth
    pub strength:   LlammaStrength,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LlammaStrength {
    /// Small band, small push (~$5k-50k crvUSD)
    Minor,
    /// Medium band ($50k-500k)
    Moderate,
    /// Large band (>$500k) — high-conviction, pre-stage immediately
    Major,
}

/// Infer LLAMMA signal from ETH price movement.
/// Called on every block where eth_price changes significantly.
pub fn infer_signal(
    eth_price_prev: f64,
    eth_price_now:  f64,
) -> Option<LlammaSignal> {
    let delta_pct = (eth_price_now - eth_price_prev) / eth_price_prev * 100.0;

    // Only interesting if ETH moved > 0.1% in one block (~$2+ at $2000 ETH)
    if delta_pct.abs() < 0.1 { return None; }

    if delta_pct < 0.0 {
        // ETH falling → LLAMMA is soft-liquidating → selling ETH, buying crvUSD
        // → crvUSD-USDC pool gets excess crvUSD → crvUSD becomes cheap
        // → Buy crvUSD on crvUSD-USDC, sell on crvUSD-USDT (if that pool is more balanced)
        tracing::debug!(
            delta_pct = delta_pct,
            "LLAMMA signal: ETH down → crvUSD excess in crvUSD-USDC pool"
        );
        Some(LlammaSignal {
            pool:          CRVUSD_USDC_POOL.parse().unwrap(),
            crvusd_excess: true,
            strength:      classify_strength(delta_pct.abs()),
        })
    } else {
        // ETH rising → LLAMMA is minting / de-liquidating → buying ETH back with crvUSD
        // → LLAMMA sells crvUSD to buy ETH → crvUSD becomes scarce in crvUSD-USDT pool
        // → buy cheap crvUSD on crvUSD-USDT (not crvUSD-USDC — that pool is the liquidation leg)
        // was incorrectly routing to CRVUSD_USDC_POOL — crvUSD-USDT is the
        // de-liquidation-tilted pool, as documented in the comment above.
        tracing::debug!(
            delta_pct = delta_pct,
            "LLAMMA signal: ETH up → crvUSD deficit in crvUSD-USDT pool"
        );
        Some(LlammaSignal {
            pool:          CRVUSD_USDT_POOL.parse().unwrap(),
            crvusd_excess: false,
            strength:      classify_strength(delta_pct.abs()),
        })
    }
}

fn classify_strength(abs_pct: f64) -> LlammaStrength {
    if abs_pct > 2.0      { LlammaStrength::Major }
    else if abs_pct > 0.5 { LlammaStrength::Moderate }
    else                  { LlammaStrength::Minor }
}
