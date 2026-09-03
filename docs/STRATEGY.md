# Strategy Specification

_Technical analysis of Curve StableSwap arbitrage mechanics, latency dynamics, and structural edge sources on Arbitrum One._

## Overview

Kingfisher captures pricing divergences between Curve StableSwap pools on Arbitrum One. When pool inventories become asymmetric, token exchange rates deviate from their target peg ratios. A single atomic transaction executes the following sequence:

1. Flash-borrows stablecoins (USDC, USDT, FRAX, crvUSD) from **Balancer V2** at **0% fee**, falling back to **Aave V3** at **5 bps fee** when Balancer capacity is constrained.
2. Executes a multi-hop swap route (1–4 hops) through the discounted pool into the premium pool.
3. Repays the flash loan principal plus any borrowing fee.
4. Enforces an on-chain profit condition via `require(netProfit >= minProfit)`.

Because the transaction is atomic, any execution that yields insufficient net profit reverts. Losing races or miscalculated spreads forfeit only transaction gas; principal is never exposed to counterparty or market risk.

## Structural Edge Sources

Plain two-pool stablecoin arbitrage during calm market regimes is highly saturated and competitive. Typical price spreads represent single-digit basis points that latency-optimized searchers close rapidly. Kingfisher's primary edge is engineered around **structural market stress**:

| Trigger | Subsystem | Mechanism | Edge Characteristic |
|---|---|---|---|
| **Peg Stress** | `edges/src/peg_stress.rs` | Chainlink USDC/USD and USDT/USD deviations > 0.25% activate stress regime. | Widens spreads across secondary pools; sizing scales into millions via spread curve. |
| **LLAMMA Cascades** | `edges/src/llamma.rs` | Soft-liquidations in Curve crvUSD lending markets push collateral into pools. | Predictable directional flow as ETH price crosses collateral liquidation bands. |
| **LP Removals** | `edges/src/lp_removal.rs` | Large `RemoveLiquidity` events instantaneously deplete single-token reserves. | Creates momentary deep mispricings before passive rebalancing occurs. |
| **Cross-Pool Cascades** | `edges/src/cascade.rs` | Severe imbalance in 2pool propagates to dependent meta-pools (FRAXBP, crvUSD). | Multi-hop triangular paths capture compounding divergence across dependent pools. |
| **Gauge Vote Windows** | `edges/src/gauge_vote.rs` | Bi-weekly Curve DAO gauge emissions weight updates alter LP incentives. | Pre-positioning around anticipated liquidity shifts and LP reallocations. |

During stress regimes, competition diminishes as generic bots fail risk checks or run out of uncollateralized borrow limits. Golden-section sizing automatically scales transaction volume to maximize profit capture without exceeding pool impact thresholds.

## Execution & Latency Architecture

Arbitrum One differs fundamentally from Ethereum L1 and PBS-governed rollups:

- **Single Sequencer & No Public Mempool**: Transactions route directly to the Offchain Labs Nitro sequencer. There is no public mempool for searchers to monitor or frontrun, and sandwich attacks cannot occur against atomic flash-loan transactions.
- **First-Come-First-Served (FCFS)**: The sequencer processes transactions in order of receipt. Physical co-location and minimal network transport latency determine competitive outcome.
- **Arbitrum Timeboost Express Lane**: Timeboost introduces a priority auction mechanism that grants express-lane sequencing rights (~200ms advantage). When configured via `TIMEBOOST_EXPRESS_LANE_URL`, Kingfisher routes high-value opportunities through priority channels rather than standard endpoints.
- **Local IPC Connectivity**: Operating alongside a local Nitro node reduces block ingestion latency from 50–250 ms (over WebSocket) down to ~0.1 ms over Unix domain sockets, maximizing the computation budget before competing transactions land.

## Sizing & Price Impact Dynamics

Curve StableSwap pools follow an invariant that blends constant-sum and constant-product behavior:

$$A \cdot n^n \sum x_i + D = A \cdot D \cdot n^n + \frac{D^{n+1}}{n^n \prod x_i}$$

As borrow volume $x$ increases, gross arbitrage yield increases linearly while marginal price impact increases non-linearly according to the amplification parameter $A$. Net profit $P(x)$ forms a strictly concave function:

$$P(x) = \text{Output}(x) - x - \text{LoanFee}(x) - \text{GasCost}$$

The sizing engine applies golden-section search over the interval $[x_{\min}, \text{ABS\_CAP\_USD}]$ to find the global maximum $x^*$ within 40–50 iterations (<300 µs), constrained by the dual-leg A-parameter gate to avoid excessive slippage.

## Authoritative On-Chain Accounting

To prevent simulation drift or false accounting, Kingfisher decouples transaction dispatch from P&L recognition:

1. **Pending Dispatch**: Broadcast transactions remain unconfirmed until receipt ingestion.
2. **Receipt Verification**: Upon inclusion (`receipt.status == 1`), the engine parses the emitted `ArbExecuted(address token, uint256 borrowAmount, uint256 grossOutput, uint256 gasUsed, uint256 netProfit)` event.
3. **P&L Crediting**: Realized P&L is credited strictly from decoded on-chain logs, never from pre-flight simulation projections.
4. **Failure Categorization**: Reverted transactions (`receipt.status == 0`) are decoded against custom error signatures (`ProfitBelowMin`, `NotBalancerVault`, `NotAavePool`, `PoolUnhealthy`). Competitive race losses (`ProfitBelowMin`) do not increment consecutive error counters.

## Ecosystem Role & Market Invariants

Kingfisher serves as an automated liquidity balancer across Arbitrum Curve pools:
- **Price Efficiency**: By closing price discrepancies between plain pools and meta-pools, it maintains uniform exchange rates for retail and institutional traders.
- **Flash Liquidity Utilization**: The protocol generates continuous fee revenue for uncollateralized lending providers (Aave V3 and Balancer V2 LPs).
- **Zero Invariant Degradation**: All swaps respect Curve's virtual price sanity invariant (`virtual_price >= 1e18`), ensuring no trade interacts with compromised or drained pools.
