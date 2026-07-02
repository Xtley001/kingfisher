# Kingfisher — Strategy

An honest description of what Kingfisher does, where its edge comes from, how it
executes on Arbitrum, and what its limits are. Written for engineers and grant
reviewers — no hype.

---

## The Trade

Kingfisher captures price discrepancies between Curve StableSwap pools on Arbitrum One.
When two pools price the same pair of stablecoins slightly differently (because one is
temporarily imbalanced), a single atomic transaction can:

1. Flash-borrow stablecoins from Aave V3 (5 bps premium).
2. Swap through the cheap pool, then the rich pool (2–4 hops).
3. Repay Aave + premium, keep the spread.

The whole thing is one transaction guarded by `require(netProfit ≥ minProfit)`. If the
spread doesn't cover the Aave premium **and** gas, the transaction reverts and the only
cost is gas. **Principal is never at risk** — there are no held deposits and no way to
finish the transaction at a loss.

---

## Where the Edge Actually Is

Plain two-pool stablecoin arbitrage in calm markets is one of the most competitive
niches in MEV. Spreads are single-digit basis points and are closed within the same
block by many latency-optimized searchers. We do not pretend otherwise: **in calm
markets, winnable, profitable trades are rare and fiercely contested.**

The durable edge is **structural stress**, and the bot is built around detecting it:

- **Peg stress** (`edges/peg_stress.rs`) — USDC/USDT depeg beyond 0.25% flips the bot
  into stress regime: it scans all pools, resizes, and fires pre-built depeg templates.
- **LLAMMA cascades** (`edges/llamma.rs`) — crvUSD soft-liquidations predictably tilt
  crvUSD pools when ETH crosses band boundaries.
- **Large LP removals** (`edges/lp_removal.rs`) — big `remove_liquidity` events create
  transient imbalances.
- **Cascade** (`edges/cascade.rs`) — a 2pool tilt often propagates to crvUSD pools.
- **Gauge-vote windows** (`edges/gauge_vote.rs`) — weekly liquidity shifts around the
  Thursday Curve gauge vote.

These events produce larger, less-contested spreads. The realistic profit profile is:
**small/occasional in calm markets, meaningful during stress events.** Sizing scales
automatically with the opportunity (golden-section search + dual-leg impact gate), so a
depeg can size into millions while a calm-market blip stays small.

---

## Execution and Latency

Arbitrum One has a **single centralized sequencer** and **no public mempool**:

- There is **no Flashbots relay** and **no PBS** for Arbitrum. Submitting bundles to
  `relay.flashbots.net` (an Ethereum-L1 service) does nothing for chain_id 42161.
- There is **no one to sandwich** an atomic flash-loan arb, so a **private mempool is
  neither necessary nor applicable**. Building one would solve a problem this strategy
  does not have.

What matters instead is **latency and ordering priority**:

1. **Broadcast** — the bot signs an EIP-1559 transaction and sends it via
   `eth_sendRawTransaction` to the lowest-latency sequencer endpoint. Whichever copy the
   sequencer sees first wins the FCFS race.
2. **Co-location + IPC** — run on bare metal near the sequencer, ideally alongside a
   local Nitro node (`--features ipc`, ~0.1ms block visibility vs 50–300ms over WS).
3. **Timeboost** — Arbitrum's express-lane auction sells ~200ms of priority sequencing.
   This is the Arbitrum-native way to *buy* first-in-line ordering during high-value
   windows. Set `TIMEBOOST_EXPRESS_LANE_URL` to route through it.

Cost note: on Arbitrum the dominant per-transaction cost is the **L1 calldata-posting
fee**, not L2 execution gas. The profit model includes an L1 component (tuned by
`L1_BASE_FEE_GWEI`) so marginal trades are not mispriced as profitable.

---

## Profitability — The Honest View

- **Downside is bounded.** Worst case per trade is wasted gas on a revert.
- **Calm-market ceiling is modest and contested.** Do not expect steady high volume
  between events.
- **Stress events are the prize.** The edge monitors exist to catch them; that is where
  the strategy earns its keep.
- **Latency decides win rate.** A cloud host far from the sequencer will lose most races;
  co-location + Timeboost is what makes the difference.

The `PERFORMANCE.md` trade-frequency figures are model projections until validated on
mainnet, and are labeled as such.

---

## Grant Positioning

Kingfisher is open-source Arbitrum MEV infrastructure that improves Curve price
efficiency and reduces slippage for all users, especially during stress events.

**Primary angle — Aave.** Kingfisher is a genuine, measurable flash-loan consumer:
every trade borrows from Aave V3 and pays the 5 bps premium, generating protocol revenue
for Aave LPs. The on-chain `ArbExecuted` event provides hard, auditable evidence of
volume and cumulative premium paid. This is the most concrete grant pitch and should
lead the application. Secondary angles: Arbitrum Foundation (ecosystem price efficiency)
and infra credits (Alchemy, Tenderly). See LAUNCH_ROADMAP.md Phase 4.
