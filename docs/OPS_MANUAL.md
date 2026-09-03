# Kingfisher — Operations Manual

## System Overview

Three running components:

```
Bare-metal server: kingfisher-bot  (Rust binary — bot engine + API server, via systemd)
Vercel / static:   dashboard       (React PWA — control interface)
Arbitrum One:      KingfisherArb   (Solidity contract — profit accumulator)
```

The bot runs continuously as a systemd service (`kingfisher.service`) on a
low-latency server co-located near the Arbitrum sequencer, ideally alongside a
local Nitro node for IPC. The dashboard connects via WebSocket. The contract
accumulates profits until the owner withdraws.

---

## Infrastructure & Node Setup

### Bare-Metal Environment
- OS: Ubuntu 22.04 LTS or newer
- Toolchains: Rust 1.80+ (`curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`), Foundry (`curl -L https://foundry.paradigm.xyz | bash && foundryup`)

### Arbitrum Nitro Node (IPC Path)
Running a local Nitro node provides IPC connectivity (~0.1ms vs 50–300ms for external RPC) and local mempool access for the Chainlink watcher.

```bash
# Clone and configure Nitro
git clone https://github.com/OffchainLabs/nitro.git
cd nitro
# Follow official documentation: https://docs.arbitrum.io/run-arbitrum-node/overview
# Configure IPC socket flag: --ipc-path=/run/nitro/arb.ipc
```

Configure `/etc/kingfisher/kingfisher.env`:
```bash
RPC_IPC_PATH=/run/nitro/arb.ipc
```

Compile with IPC feature flag:
```bash
cargo build --release --features ipc
```

---

## Daily Operations

### Status Check (30 seconds)
```
1. Open dashboard
2. Verify: green connection dot (top right)
3. Verify: block numbers incrementing
4. Verify: gas tank > 30%
5. Verify: no red alerts in status bar
```

### Morning Review (5 minutes)
```
1. Dashboard → check today's P&L and trade count
2. journalctl -u kingfisher --since "24h ago" | grep ERROR → scan for errors
3. Gas tank → if < 50%, plan a refill
4. Win rate → if < 60% over past 24h, review revert reasons
5. Aave status → confirm reserve_active = true
```

---

## Starting the Bot

systemd auto-starts the service on boot and restarts it on failure. Manual control:
```bash
sudo systemctl start kingfisher     # start
sudo systemctl restart kingfisher   # restart
sudo systemctl status kingfisher    # check state
journalctl -u kingfisher -f         # follow logs
```
After a code change, rebuild then restart:
```bash
cd bot && cargo build --release --features ipc
sudo cp bot/target/release/kingfisher /usr/local/bin/kingfisher
sudo systemctl restart kingfisher
```

If the bot was paused via dashboard:
```
Dashboard → top right → "Resume Bot"
```

---

## Stopping the Bot

**Stop bot engine only (service keeps running, no new txs):**
```
Dashboard → Kill Switch → "Running — Pause" → confirm
Resume any time via dashboard.
```

**Stop bot + prevent any new txs landing (on-chain pause):**
```bash
cast send $CONTRACT_ADDRESS_MAINNET \
  "setPaused(bool)" true \
  --ledger \
  --rpc-url $RPC_HTTP_URL
# Even if the bot submits a tx, it reverts with "KF: paused"
```

**Full stop + emergency withdrawal:**
See: Emergency Shutdown section below.

---

## Gas Management

### Target: 1.0 ETH in bot wallet at all times

| Level | ETH Balance | System Behavior |
|---|---|---|
| Healthy | > 0.30 ETH | Normal operation |
| Alert | 0.10–0.30 ETH | Telegram alert fires, bot continues |
| Critical | < 0.10 ETH | Bot auto-halts, Telegram alert fires |

### Refill Procedure
```
1. Get bot wallet address:
   cast wallet address $BOT_PRIVATE_KEY

2. Send ETH from cold wallet to bot wallet
   Target: top up to 0.5–1.0 ETH

3. Dashboard gas tank updates on next block (~250ms)

4. If bot was halted: it auto-resumes when balance > gas_reserve_eth
   (Or click Resume if it doesn't trigger within 2 blocks)

Note: NEVER send stablecoins to the bot wallet.
      Profits accumulate in the contract, not the bot wallet.
```

---

## Profit Withdrawal

Profits accumulate in KingfisherArb as USDC/stablecoins.
Withdraw to cold wallet at least weekly.

### Method A — Dashboard One-Click (Operator Hot Wallet)
Click the `↑ Withdraw` button in the top navigation bar of the dashboard and confirm.
The bot will submit a direct `withdrawProfitBatch` transaction to sweep all accumulated
stablecoin balances directly to the contract owner (cold wallet).

### Method B — Direct from Cold Wallet (Recommended for Cold Transfers)
```bash
# Check contract balance
cast call $CONTRACT_ADDRESS_MAINNET \
  "balanceOf(address)(uint256)" \
  $CONTRACT_ADDRESS_MAINNET \
  --rpc-url $RPC_HTTP_URL

# Withdraw USDC to owner (cold wallet)
cast send $CONTRACT_ADDRESS_MAINNET \
  "withdrawProfit(address)" \
  $USDC_ADDRESS \
  --ledger \
  --rpc-url $RPC_HTTP_URL

# Withdraw multiple tokens at once
cast send $CONTRACT_ADDRESS_MAINNET \
  "withdrawProfitBatch(address[])" \
  "[$USDC_ADDRESS,$USDT_ADDRESS,$FRAX_ADDRESS]" \
  --ledger \
  --rpc-url $RPC_HTTP_URL
```

### Withdrawal Schedule
```
Minimum:          Weekly (Sundays)
After stress event: Same day
After any anomaly:  Before resuming
Target:           Never hold > 1 week's earnings in the contract
```

---

## Authoritative P&L & Landing Tracker

Kingfisher completely decouples transaction submission from P&L recognition:
1. **Submission**: When the Arbitrum sequencer accepts a transaction, it is marked **pending** (`success: false`, `profit_usd: None`). It does NOT credit profit, increment trade counts, or reset the circuit breaker.
2. **Landing Confirmation**: The Landing Tracker polls `eth_getTransactionReceipt`:
   - If `receipt.status == 1`: The bot decodes the on-chain `ArbExecuted(address,uint256,uint256,uint256,uint256)` event emitted by `KingfisherArb`. Net profit is credited exclusively from this event (falling back to $0.0 if missing, never to pre-trade simulation estimates). The trade is recorded as landed, and `consecutive_reverts` is reset to 0.
   - If `receipt.status == 0`: The transaction reverted on-chain. It is classified via `RevertClass` (e.g. `ProfitBelowMin`, `PoolUnhealthy`, `PoolNotAllowed`). Non-race-loss errors increment `consecutive_reverts` and count toward the circuit breaker.
   - If expired (3 blocks past target with no receipt): The bundle is dropped and recorded as unincluded.
3. **Persistence**: Every confirmed landed trade and revert is appended to `{KINGFISHER_DATA_DIR}/trades.jsonl` via `append_trade()`. On process restart, `load_history()` restores all-time metrics.
4. **Gas Accounting**: Real gas cost in USD (`receipt.gas_used * receipt.effective_gas_price * eth_price`) is accumulated in `total_gas_spent_usd` and monitored by the 1-hour gas drain watchdog.

---

## Parameter Tuning

### How to Change Parameters
```
Dashboard → Parameters page → edit value → Apply
Changes apply on the next block scan — no restart required.
Parameters persist to {KINGFISHER_DATA_DIR}/params.json (default
/var/lib/kingfisher/params.json) across restarts.
```

### Tuning Guide

| Signal | Likely Cause | Adjustment |
|---|---|---|
| Very few trades (<10/day) | Min profit too high or imbalance threshold too strict | Lower min_profit_usd to 50, or min_imbalance_pct to 3.5 |
| Win rate < 60% | Simulation inaccuracy or high competition | Raise min_profit_usd to 100 |
| High revert rate | Gas spike, simulation drift, or competition | Check eth_call validation logs, raise min_profit_usd |
| Many small profits | Normal — expected | No action |

### Parameter Limits

```
min_profit_usd:    $20 minimum  · $500 maximum
min_imbalance_pct: 2.0 minimum  · 10.0 maximum
gas_reserve_eth:   0.05 minimum (recommended 0.10)
alert_gas_eth:     always > gas_reserve_eth
abs_cap_usd:       $1B API ceiling — sizing is automatic from spread curve
```

---

## Adding a New Pool

New pools discovered by the factory monitor are added to the watch list but
NOT the trading list. Manual approval is required.

### Approval Process
```
1. Receive Telegram alert: "🆕 New Curve pool detected: 0x..."

2. Verify the pool on Arbiscan:
   - Is it a Curve pool? (Check factory, source code)
   - Stablecoins only? (Kingfisher targets stablecoin pools)
   - Who deployed it? (Curve factory = trusted; EOA = suspicious)

3. Monitor for 24 hours:
   - Watch balance history in dashboard
   - Confirm virtual_price >= 1e18 and stable.
     *Note on Pool Health vs Slippage*: `isPoolHealthy()` (`virtual_price >= 1e18`) serves strictly as a coarse sanity floor against drained/broken Curve pools. Actual frontrunning and sandwich protection is enforced by the mathematical per-hop `minAmountOut` dynamic slippage model in the calldata encoder.
   - Review imbalance patterns

4. Add to contract allowlist (from cold wallet):
   cast send $CONTRACT_ADDRESS \
     "setPoolAllowed(address,bool)" \
     <pool_address> true \
     --ledger \
     --rpc-url $RPC_HTTP_URL

5. Add pool config to bot (requires code change + redeploy):
   - Edit bot/crates/core/src/config.rs → mainnet_pools()
   - Add PoolConfig with correct token addresses and decimals
   - Verify A-parameter from pool.A() on-chain — never guess
   - Rebuild and restart: cargo build --release --features ipc && sudo systemctl restart kingfisher

6. Monitor first 48h of trading on new pool:
   - Win rate should be > 70% (new pools have less competition)
   - Watch for unexpected reverts
```

### Pool Config Template
```rust
// In bot/crates/core/src/config.rs → mainnet_pools()
PoolConfig {
    name:    "TOKEN_A-TOKEN_B".into(),
    address: "0x...".parse().unwrap(),  // Verified on Arbiscan
    tokens: vec![
        TokenConfig {
            symbol:   "TOKEN_A".into(),
            address:  "0x...".parse().unwrap(),  // From pool.coins(0)
            decimals: 18,   // Verify: USDC=6, USDT=6, FRAX=18, crvUSD=18
            index:    0,
        },
        TokenConfig {
            symbol:   "TOKEN_B".into(),
            address:  "0x...".parse().unwrap(),  // From pool.coins(1)
            decimals: 6,
            index:    1,
        },
    ],
    is_meta: false,   // true only if pool has exchange_underlying()
    priority: 1,      // 1=always watch, 2=stress only
},
```

---

## Incident Runbooks

### Bot Shows "Disconnected" on Dashboard
```
Diagnosis:
  1. Check logs: journalctl -u kingfisher -f
  2. API server crashed → sudo systemctl restart kingfisher
  3. WebSocket error → check RPC / IPC connection
  4. Repeated → check RPC provider rate limits

Resolution:
  - Transient disconnect: dashboard auto-reconnects in <30s
  - API crash: sudo systemctl restart kingfisher
  - RPC issue: check your Nitro node / Alchemy / QuickNode for an outage
```

### 5 Consecutive Reverts (Auto-Paused)
```
Telegram alert: "⚠️ 5 consecutive tx failures — Kingfisher auto-paused"

Check logs (journalctl -u kingfisher) for the custom-error revert reason:
  ProfitBelowMin(got,min) → Simulation overestimating / lost the race. Wait 10 min,
                            Resume. If repeating: raise min_profit_usd slightly.
  NotAavePool()           → Security issue. DO NOT RESUME. Check Aave status.
  PoolNotAllowed(pool)    → Route using an unapproved pool — fix route_graph.rs.
  PoolUnhealthy(pool)     → Pool virtual price < 1e18. Check Curve Discord.
  Gas errors              → Gas spike (brief on Arbitrum). Resume.
```

### Aave Reserve Frozen or Paused
```
Telegram alert: "🚨 CRITICAL: Aave USDC reserve frozen — Bot auto-halted"

This is a protocol-level event, not a Kingfisher bug.

  1. Check https://governance.aave.com and @aave on Twitter
  2. Wait for Aave to unpause the reserve
  3. Bot auto-resumes when reserve becomes active

Do NOT manually override — there may be a security reason for the pause.
Profits in the contract are safe — Aave freeze does not affect token balances.
```

### Gas Critical (Bot Halted)
```
Telegram alert: "⛽ Gas CRITICAL — Balance: X ETH — Floor: 0.10 ETH — Bot halted"

Resolution (5 minutes):
  1. Send ETH to bot wallet (cast wallet address $BOT_PRIVATE_KEY)
  2. Target: reach 0.5–1.0 ETH total
  3. Bot auto-resumes when balance > gas_reserve_eth
```

### Virtual Price Drop Alert
```
Telegram alert: "⚠️ VP drop on FRAX-USDC"

A pool's virtual price dropped below 1e18 — possible exploit or critical bug.

Immediate actions:
  1. Dashboard → Kill Switch → Pause
  2. Do NOT withdraw profits yet — wait for clarity
  3. Check Curve Discord for the pool in question
  4. Check Twitter/DeFiLlama for hack reports

If exploit confirmed:
  1. Remove pool from allowlist (cold wallet)
  2. Check if profits are denominated in the affected stablecoin
  3. Withdraw unaffected tokens to cold wallet
  4. Resume with remaining pools only

If false alarm (reorg or data error):
  1. Wait 2 minutes — virtual price should recover
  2. Click Resume
```

### Simulation Divergence > 0.1% (Auto-Paused)
```
Telegram alert: "🛑 Kingfisher AUTO-PAUSED: sim drift X% at block N"

The algebraic fast-path is giving different results from the live eth_call.

Common causes:
  1. Pool A-parameter ramping (governance vote in progress)
     → Ensure A() is queried live on every block, not cached
  2. Pool is a metapool (exchange_underlying vs exchange)
     → Verify is_meta flag matches the pool
  3. Decimal normalization error
     → Verify token decimals in pool config match ERC20.decimals()
  4. Pool fee not read from chain
     → Confirm fee() is in the multicall ABI (fee_rate must not fall back to default)

Resolution:
  - Fix the root cause
  - Run: cargo test -p kingfisher-simulation validation
  - When divergence < 0.1%: Resume via dashboard
```

---

## Monthly Review

Do this on the first of each month (30 minutes).

```
Performance
  □ Total P&L vs last month
  □ Average profit per trade (target: >$150 calm, >$1000 during events)
  □ Win rate (target: >70%)
  □ Gas spend (target: <0.5% of gross profit)
  □ Number of stress events and P&L during them

Operations
  □ Uptime percentage (target: >99%)
  □ How many auto-pauses? Why?
  □ Any incidents? Resolved?
  □ New pools worth adding?

Competition
  □ Win rate declining? (New competitor on your routes)
  □ If win rate <60%: raise min_profit_usd to filter marginal trades

Maintenance
  □ Withdraw accumulated profits to cold wallet
  □ cargo update in bot/ (run cargo test --all after)
  □ npm update in dashboard/ (test after)
  □ Review Aave governance: any upcoming votes on USDC reserve?
  □ Review Curve governance: any pool upgrades or new deployments?
  □ Rotate API key (recommended quarterly)
```

---

## Emergency Shutdown (Full)

Use when you need to stop everything and secure all funds immediately.

```bash
# Step 1: Pause bot via dashboard
# Dashboard → Kill Switch → Pause

# Step 2: Pause contract on-chain
cast send $CONTRACT_ADDRESS_MAINNET \
  "setPaused(bool)" true \
  --ledger \
  --rpc-url $RPC_HTTP_URL

# Step 3: Withdraw all profits (unpause first — withdraw requires unpaused)
cast send $CONTRACT_ADDRESS_MAINNET \
  "setPaused(bool)" false \
  --ledger \
  --rpc-url $RPC_HTTP_URL

cast send $CONTRACT_ADDRESS_MAINNET \
  "withdrawProfitBatch(address[])" \
  "[0xaf88d065e77c8cC2239327C5EDb3A432268e5831,0xFd086bC7CD5C481DCC9C85ebE478A1C0b69FCbb9,0x17FC002b466eEc40DaE837Fc4bE5c67993ddBd6F]" \
  --ledger \
  --rpc-url $RPC_HTTP_URL

# Step 4: Re-pause contract
cast send $CONTRACT_ADDRESS_MAINNET \
  "setPaused(bool)" true \
  --ledger \
  --rpc-url $RPC_HTTP_URL

# Step 5: Withdraw gas ETH from bot wallet to cold wallet manually

# Step 6: Stop the service
sudo systemctl stop kingfisher

# System is now fully stopped and all funds secured.
# To restart: sudo systemctl start kingfisher, unpause contract, resume via dashboard.
```
