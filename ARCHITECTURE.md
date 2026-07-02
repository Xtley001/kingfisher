# Kingfisher — Architecture

## System Diagram

```
Arbitrum One chain
      │
      ├─ IPC (~0.1ms) ──── Kingfisher Bot (Rust)
      └─ WebSocket (fallback)          │
                                       ├─ 5-Layer Scanner
                                       │    L1: Pool imbalance filter (% off center)
                                       │    L2: Velocity filter (imbalance Δ per block)
                                       │    L3: Algebraic simulation + gas gate
                                       │    L4: Route graph (DFS / Bellman-Ford)
                                       │    L5: Algebraic fast-path validation vs eth_call
                                       │
                                       ├─ Sizing Engine
                                       │    - Golden-section search on profit curve
                                       │    - Dual-leg A-parameter impact gate
                                       │    - Hard ceiling: $25M HARD_CAP_USD
                                       │
                                       └─ Executor
                                            - EIP-1559 tx builder
                                            - eth_sendRawTransaction → Arbitrum sequencer
                                              (Timeboost express lane if configured)
                                            - Landing tracker (receipt confirmation)
                                                  │
                                       ┌──────────┘
                                       ▼
                              KingfisherArb.sol (Arbitrum One)
                                       │
                                       ├─ Aave V3 flashLoanSimple()
                                       ├─ Curve pool route execution (1–4 hops)
                                       ├─ Profit guard (require netProfit ≥ minProfit)
                                       └─ Profit accumulates → cold wallet withdrawal
```

---

## Crate Responsibilities

**`kingfisher-core`** — shared types, configuration, and state. Defines `Network`, `PoolConfig`, `TokenConfig`, `BotParams`, `Opportunity`, and `BotState`. All other crates depend on this; it has no internal dependencies. The single canonical Curve StableSwap invariant solver lives in `kingfisher-simulation` (`spread.rs`) and is used throughout the scanner, sizing engine, and edge monitors.

**`kingfisher-chain`** — block loop and on-chain data ingestion. Prefers IPC connection to a co-located Arbitrum Nitro node; falls back to WebSocket. Runs the Chainlink oracle watcher, multicall pool state fetcher, and event indexer for new pool discovery. Publishes `BotState` updates to the shared `RwLock`.

**`kingfisher-scanner`** — 5-layer opportunity filter. L1 filters by pool imbalance percentage, L2 by velocity (imbalance change per block), L3 runs the algebraic cost simulation, L4 searches the route graph via DFS and Bellman-Ford for multi-hop paths, and L5 validates profitable candidates against eth_call. Returns `Vec<Opportunity>` sorted by expected profit.

**`kingfisher-simulation`** — profit simulation and validation. `simulate_opportunity()` is the algebraic fast-path used by the scanner (pure math, no RPC). `validation.rs` periodically compares the fast-path result to an eth_call result to detect simulation drift. `sizing.rs` implements the golden-section search for optimal flash loan amount.

**`kingfisher-edges`** — edge-case opportunity monitors. Watches for LLAMMA liquidation cascades, peg stress events (USDC/USDT depeg), gauge vote windows (Curve emissions shifts), new pool deployments via Curve Factory events, LP removal events that create transient imbalances, Convex pool parameter changes, and admin fee collection events.

**`kingfisher-executor`** — transaction building and submission. Builds EIP-1559 transactions with the encoded `executeArb()` calldata, signs them with the bot (operator) wallet, and broadcasts via `eth_sendRawTransaction` to the Arbitrum sequencer — or the Timeboost express lane if `TIMEBOOST_EXPRESS_LANE_URL` is set — with an optional best-effort mirror to a backup endpoint. There is no Flashbots/PBS on Arbitrum (single sequencer, no public mempool), so no bundle or builder-ranking layer is needed.

**`kingfisher-api`** — REST + WebSocket API for the dashboard. Exposes bot state, opportunity feed, parameter updates, kill switch, and Prometheus metrics. Protected by static API key authentication.

---

## 5-Layer Scanner

Each block triggers the following pipeline for all configured pool pairs:

| Layer | Name | Action | Rejection reason |
|---|---|---|---|
| L1 | Imbalance filter | Compute `|balance_ratio - 1.0|` for each token pair | `< min_imbalance_pct` (default 5%) |
| L2 | Velocity filter | Compare imbalance to previous block | `< min_velocity` (default 0.015) — stale imbalance |
| L3 | Algebraic sim + gas gate | Run `simulate_opportunity()` | `net_profit < effective_min_profit` |
| L4 | Route graph | DFS over pool graph for multi-hop paths | No profitable path found |
| L5 | Validation | Compare algebraic result to eth_call | Divergence > 0.1% triggers auto-pause |

In normal markets, L1 and L2 eliminate ~90% of candidates before any simulation runs.

---

## Sizing Engine

The sizing engine finds the optimal flash loan amount for a given route in four steps:

**Step 1 — Profit curve shape.** The net profit function `P(x)` is concave for Curve StableSwap pools: it rises as borrow amount `x` increases (more arb captured), then falls as price impact exceeds the spread. There is a unique global maximum.

**Step 2 — Golden-section search.** Search the interval `[min_flash, HARD_CAP_USD]` using the golden-section method. This converges to the profit-maximising `x` in `O(log n)` iterations without computing derivatives. Typical convergence: 40–50 iterations.

**Step 3 — A-parameter impact gate.** After finding the unconstrained optimum, apply the dual-leg A-parameter check. The Curve amplification parameter `A` determines how much price impact a given trade size causes. If the optimum trade moves the pool's virtual price by more than the configured threshold, scale back to the impact-bounded size.

**Step 4 — Hard cap.** The final amount is `min(optimum, ABS_CAP_USD, HARD_CAP_USD)`. `HARD_CAP_USD = $25M` is a compile-time constant in `sizing.rs`. `ABS_CAP_USD` is the operator-configurable ceiling (default $5M, tunable via dashboard after observing live P&L).

---

## Edge Monitors

**LLAMMA** (`edges/src/llamma.rs`) — monitors Curve's LLAMMA lending market for liquidation bands entering the active price range. Liquidations create transient imbalances in the associated stablecoin pools.

**Peg stress** (`edges/src/peg_stress.rs`) — tracks Chainlink USDC/USD and USDT/USD feeds. When either depegs beyond 0.25%, the bot enters stress regime: scanner runs on all pools (not just priority-1), position sizing is reduced, and alerts fire. Exits stress regime when peg recovers to within 0.15% (hysteresis prevents flapping).

**Gauge vote window** (`edges/src/gauge_vote.rs`) — watches Curve gauge controller for large CRV emissions shifts. Major gauge weight changes alter pool incentives and can precede significant LP flow changes.

**New pool** (`edges/src/new_pool.rs`) — watches the Curve Factory for `PlainPoolDeployed` and `MetaPoolDeployed` events. New pools enter a 24-hour monitoring window before becoming arb-eligible. The keccak256 topic hash `0xe6e1b7d...` identifies plain pool events vs meta pool events.

**Cascade** (`edges/src/cascade.rs`) — detects correlated imbalances across multiple pools (e.g., simultaneous stress in FRAX-USDC and crvUSD-USDC) that may indicate a broader depeg event rather than isolated arbitrage.

**LP removal** (`edges/src/lp_removal.rs`) — large LP `remove_liquidity` events create immediate imbalances. Monitors the mempool (IPC path) for pending removals to front-run the resulting arb window.

**Depeg templates** (`edges/src/templates.rs`) — pre-built route templates for known depeg scenarios (e.g., FRAX soft peg deviation). Avoids cold-start latency on known event patterns.

---

## IPC vs RPC

| Connection | Latency | Use case |
|---|---|---|
| IPC (co-located Nitro) | ~0.1ms | Production — full mempool, lowest latency |
| WebSocket (Alchemy/QuickNode) | 50–300ms | Cloud deployment or IPC unavailable |
| HTTP (fallback) | 100–500ms | Simulation and validation calls only |

IPC is the difference between seeing a block in 0.1ms and 150ms. On Arbitrum's ~250ms block time, that is a meaningful fraction of the available window. The Chainlink mempool monitor (`chainlink_watch.rs`) requires IPC to observe pending oracle update transactions before they land — without it, Chainlink price updates are observed only after confirmation.

To build with IPC support:
```bash
cargo build --release --features ipc
```

Confirm `RPC_IPC_PATH` is set and the socket exists before enabling. See `LAUNCH_ROADMAP.md` Phase 1.2 for Nitro node setup.
