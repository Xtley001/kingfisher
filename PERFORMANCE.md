# Kingfisher — Performance

## Block Pipeline Latency

| Path | Target p99 | Measured |
|---|---|---|
| IPC (co-located Nitro node) | < 0.1ms | TBD — post-mainnet |
| WebSocket (Alchemy/QuickNode) | < 300ms | TBD |
| Full block → submission | < 400ms | TBD |

Benchmarks marked TBD will be filled with real numbers after the first 30 days on mainnet.

## Scanner Throughput

- 4 pools × 6 directed token pairs = 24 routes evaluated per block
- 5-layer filter reduces 24 candidates to < 3 viable per block in normal markets
- Sizing engine runtime: < 1ms (algebraic, no RPC calls)
- Edge monitors run in parallel with the main scanner pass

## Gas Targets

| Route | Gas budget | Measured |
|---|---|---|
| 2-hop | < 400,000 | TBD |
| 3-hop | < 550,000 | TBD |
| 4-hop | < 700,000 | TBD |

`GAS_LIMIT_OVERRIDE` in `.env` defaults to 750,000 — accommodates 4-hop routes through
meta-pools with headroom. Tune down after collecting real measurements.

> **Cost note (Arbitrum):** the dominant per-transaction cost is the **L1
> calldata-posting fee**, not L2 execution gas. The profit model adds an L1 component
> (see `gas_usd_for_route` in `simulation/src/lib.rs`), tuned by `L1_BASE_FEE_GWEI`.
> A stale L1 estimate is caught by the `eth_call` divergence validator before it can
> cause sustained losses.

## Expected Trade Frequency

- Normal markets: 18–35 profitable opportunities/day *(model projection, unvalidated)*
- Peg stress events: 50–200+/day *(model projection, unvalidated)*
- Source: sizing-model projections only. Calm-market stablecoin arb is highly contested;
  treat these as upper-bound estimates until replaced with live mainnet data. The
  durable edge is stress events — see docs/STRATEGY.md.

## Sizing Engine

The golden-section search converges in approximately 40–50 iterations.
Each iteration calls `simulate_opportunity()` once (pure math, ~5µs).
Total sizing decision: < 300µs per candidate opportunity.

## Simulation Validation

The algebraic fast-path vs eth_call divergence check runs every N blocks (configurable).
Expected divergence in normal conditions: < 0.01%.
Auto-pause threshold: 0.1% (configurable in `main.rs`).
