# Kingfisher — Testing Protocol

## Philosophy

No trade touches mainnet until every phase below passes. The cost of a buggy
flash loan on Arbitrum mainnet is gas only (the tx reverts atomically), but a
contract bug that bypasses the profit guard could drain accumulated profits.
The test suite is the only way to prove the profit guard and callback security
work correctly.

---

## Phase 0 — Unit Tests (No Network, No Wallet)

Pure Rust logic tests. Run in any environment instantly.

```bash
cd bot
cargo test --all
```

### What Must Pass

```
kingfisher-simulation sizing::tests
  ✓ deep_imbalanced_pool_sizes_large
  ✓ higher_a_allows_larger_impact
  ✓ impact_threshold_monotone
  ✓ impact_threshold_in_bounds
  ✓ golden_section_quadratic
  ✓ golden_section_plateau
  ✓ bidirectional_finds_direction
  ✓ gas_cost_reduces_size
  ✓ hard_cap_respected
  ✓ aave_cap_respected
  ✓ balanced_pool_returns_zero
  ✓ clear_imbalance_returns_positive
  ✓ both_leg_gate_constrains_on_shallow_exit
  ✓ impact_gate_allows_large_deep_pool_trade
  ✓ impact_gate_restricts_thin_pool
  ✓ stress_event_sizes_into_millions

kingfisher-scanner filters::tests
  ✓ test_imbalance_filter_pass
  ✓ test_imbalance_filter_fail
  ✓ test_velocity_filter_pass
  ✓ test_velocity_filter_fail

kingfisher-core
  ✓ test_bot_params_defaults
  ✓ test_effective_min_profit_usd
  ✓ test_bot_params_from_env_overrides
  ✓ test_network_chain_ids
  ✓ test_network_from_env_defaults_to_testnet
  ✓ test_race_loss_does_not_increment_consecutive_reverts
  ✓ test_bug_revert_increments_consecutive
  ✓ test_success_resets_consecutive
  ✓ test_daily_reset_clears_today_counters
  ✓ test_total_profit_not_affected_by_daily_reset
  ✓ test_win_rate_uses_all_time_reverts
  ✓ test_gas_regime_transitions
```

---

## Phase 1 — Foundry Fork Tests (Arbitrum Mainnet State, Zero Real Funds)

Real contracts, real pool data, no money at risk. Every test must pass.

### Setup
```bash
cd contracts

forge install foundry-rs/forge-std --no-git
forge install aave/aave-v3-core --no-git
forge install OpenZeppelin/openzeppelin-contracts --no-git

set -a && source ../.env.testnet && set +a

# Run all fork tests
forge test --fork-url $RPC_HTTP_URL -vvvv

# Run with gas report
forge test --fork-url $RPC_HTTP_URL --gas-report
```

### Mandatory Checklist

```
Contract Deployment
  ✓ test_Deployment                  — correct addresses, owner, params
  ✓ Aave Pool address matches network
  ✓ All 4 primary pools in allowlist

Access Control
  ✓ test_OnlyOwnerCanExecute         — non-owner reverts correctly
  ✓ test_PausedReverts               — paused contract reverts ContractPaused()
  ✓ test_AaveCallbackOnlyFromAave    — wrong sender reverts NotAavePool()
  ✓ test_AaveCallbackBadInitiator    — wrong initiator reverts BadInitiator()

Pool Security
  ✓ test_UnallowedPoolReverts        — unallowed pool in route reverts
  ✓ test_PoolHealthy                 — isPoolHealthy() returns true for live pools
  ✓ test_UnhealthyPoolBlocked        — pool with VP < 1e18 is blocked

Profit Guard
  ✓ test_ProfitGuardReverts          — no profit → tx reverts
  ✓ test_MinProfitRespected          — require(netProfit >= minProfit) works

Operations
  ✓ test_WithdrawProfit              — owner can withdraw accumulated tokens
  ✓ test_WithdrawProfitBatch         — batch withdrawal works
  ✓ test_WithdrawETH                 — ETH withdrawal works
  ✓ test_SetMinProfit                — min profit update emits event
  ✓ test_SetPaused                   — pause/unpause works
  ✓ test_SetPoolAllowed              — allowlist management works

Gas Targets
  ✓ 2-hop route: < 400,000 gas
  ✓ 3-hop route: < 550,000 gas
  ✓ 4-hop route: < 700,000 gas
```

### Simulate a Real Arb (Mainnet Fork)
```bash
forge test --fork-url $RPC_HTTP_URL --match-test test_RealArb -vvvv

# Expected output:
# Pool imbalance created: FRAX-USDC at ~60/40
# Flash loan: 1,000,000 USDC from Aave V3
# Aave fee: 500 USDC (0.05%)
# Net profit: > $0
# ✓ ArbExecuted event emitted
# ✓ require(profit >= minProfit) passed
```

---

## Phase 2 — Arbitrum Sepolia Testnet

Full system test with real deployments. No real money.

### Deploy Contract to Sepolia
```bash
cd contracts
set -a && source ../.env.testnet && set +a

forge script script/DeployTestnet.s.sol \
  --rpc-url $RPC_HTTP_URL \
  --broadcast --verify \
  --etherscan-api-key $ARBISCAN_KEY \
  -vvvv

# Copy the printed address → CONTRACT_ADDRESS_TESTNET=0x... → .env.testnet
```

### Get Testnet ETH
```
Arbitrum Sepolia faucets:
  https://faucet.triangleplatform.com/arbitrum/sepolia
  https://faucets.chain.link/arbitrum-sepolia

Fund bot wallet with 0.1 ETH for gas testing.
```

### Run Bot on Testnet
```bash
cd bot
set -a && source ../.env.testnet && set +a
NETWORK=testnet RUST_LOG=kingfisher=debug cargo run --bin kingfisher
```

### Testnet Checklist
```
Bot startup
  ✓ Reads NETWORK=testnet
  ✓ Loads Sepolia Aave Pool address correctly
  ✓ Connects to Sepolia WebSocket RPC
  ✓ Block numbers appear in logs
  ✓ Logs are JSON format
  ✓ API server starts on port 3001
  ✓ Startup pool config audit logged (A, impact%, fee_source per pool)

Dashboard connectivity
  ✓ Connects to ws://localhost:3001/ws?key=<key>
  ✓ TESTNET badge shows in amber
  ✓ Block numbers tick in real time
  ✓ ETH price field populates (Chainlink)
  ✓ Wallet ETH balance shows correctly

Dashboard controls
  ✓ Kill switch PAUSE → bot stops processing blocks
  ✓ Kill switch RESUME → bot resumes
  ✓ Parameter update applies on next block
  ✓ Parameter change persists after bot restart

Gas management
  ✓ Gas alert fires when balance < alert_gas_eth
  ✓ Bot halts when balance < gas_reserve_eth
  ✓ Telegram alert fires on critical gas
```

---

## Phase 3 — Local anvil Fork (Full Integration)

`anvil --fork-url` mirrors Arbitrum mainnet state locally. No rate limits.
Infinite test ETH. Closest to mainnet conditions without spending real money.

### Start anvil Fork
```bash
# Terminal 1: start anvil
set -a && source ../.env.testnet && set +a

anvil \
  --fork-url $RPC_HTTP_URL \
  --chain-id 42161 \
  --port 8545 \
  --accounts 10 \
  --balance 1000 \
  --block-time 0

# Terminal 2: run bot against local fork
cd bot
set -a && source ../.env.testnet && set +a

RPC_WS_URL=ws://localhost:8545 \
RPC_HTTP_URL=http://localhost:8545 \
NETWORK=testnet \
RUST_LOG=kingfisher=debug \
cargo run --bin kingfisher
```

### Create Imbalance to Trigger a Real Arb
```bash
# Terminal 3: whale swap to create imbalance in FRAX-USDC
WHALE=0xB38e8c17e38363aF6EbdCb3dAE12e0243582891D
FRAX_USDC=0x0c9b8A3FDECb9d5B218D02555a8BaF332e5b740d
USDC=0xaf88d065e77c8cC2239327C5EDb3A432268e5831

cast rpc anvil_impersonateAccount $WHALE
cast send $USDC \
  "approve(address,uint256)" $FRAX_USDC 500000000000 \
  --from $WHALE --rpc-url http://localhost:8545 --unlocked

cast send $FRAX_USDC \
  "exchange(int128,int128,uint256,uint256)" 1 0 500000000000 0 \
  --from $WHALE --rpc-url http://localhost:8545 --unlocked

cast rpc anvil_mine 1

# Watch bot logs:
# "🎯 Best opportunity"
# "📡 Broadcasting arb transaction to sequencer"
```

### Integration Assertions
```
After 30 minutes on anvil fork:
  ✓ At least 1 profitable opportunity detected
  ✓ Signed transaction constructed and broadcast-attempted (logged)
  ✓ Algebraic simulation completes without panic
  ✓ Profit estimate and simulation result within 10%
  ✓ No memory leaks (process RSS stable over time)
  ✓ API WebSocket stream uninterrupted
  ✓ Dashboard connected continuously
```

---

## Phase 4 — Simulation Accuracy Validation (algebraic vs eth_call)

The algebraic fast-path must agree with the on-chain `eth_call` within 0.1%.

```bash
cargo test -p kingfisher-simulation validation -- --nocapture
```

### Expected Output
```
Test route: USDC → FRAX (FRAX-USDC) → USDC (2pool)
Flash amount: $500,000

algebraic result: net_profit = $847.23
eth_call result:  net_profit = $848.11
Divergence: 0.10%  ← must be < 0.1%

PASS ✓
```

### If Divergence > 0.1%
```
Common causes:
  1. A-parameter read from cache instead of on-chain
     → Query A() live on every block
  2. Pool is a metapool — wrong exchange function used
     → Verify is_meta flag is correct
  3. Decimal normalization mismatch
     → Verify token decimals in pool config match ERC20.decimals()
  4. Pool fee not read from chain
     → Confirm fee() is in the multicall ABI (fee_rate must not default)
  5. Fee calculation wrong
     → Aave fee = amount × 5 / 10000
```

---

## Phase 5 — Stress Regime Simulation

Simulate a USDC depeg event to verify the bot handles it correctly.

```solidity
// In test/StressRegime.t.sol
function test_StressRegimeActivates() public {
    // Mock Chainlink USDC/USD to return $0.997 (0.3% depeg)
    vm.mockCall(
        0x50834F3163758fcC1Df9973b6e91f0F0F0434aD3,
        abi.encodeWithSignature("latestRoundData()"),
        abi.encode(
            uint80(1),
            int256(99700000),    // $0.997 (8 decimals)
            uint256(block.timestamp),
            uint256(block.timestamp),
            uint80(1)
        )
    );
    // Verify: stress_regime = true, sizing uses spread-curve optimal,
    // depeg templates fire immediately
}
```

### Stress Test Assertions
```
  ✓ stress_regime = true when |peg - 1.0| > 0.002
  ✓ stress_regime = false after peg returns to normal
  ✓ Telegram alert fires on stress detection
  ✓ Bot does not crash during stress regime
  ✓ Optimal sizing computed from spread curve (not hardcoded)
  ✓ Depeg templates exist and are not stale
  ✓ Templates refresh every 100 blocks
```

---

## Phase 6 — Pre-Production Go/No-Go Checklist

All items must be checked before deploying to mainnet.

### Smart Contract
```
□ All Foundry fork tests passing (zero failures)
□ Gas usage within targets: 2-hop <400k, 3-hop <550k, 4-hop <700k
□ Contract verified on Arbiscan (source code visible)
□ Emergency pause tested (setPaused(true) blocks executeArb)
□ Withdrawal tested (withdrawProfit sends to owner)
□ Min profit guard tested (tx reverts if not profitable)
□ Both Aave security checks present:
    caller == address(AAVE_POOL)  else revert NotAavePool()
    initiator == address(this)    else revert BadInitiator()
□ Pool allowlist populated with all 4 primary pools
□ Owner is cold wallet address (NOT bot wallet)
□ Contract tested for 72h on anvil fork without errors
□ No approve(address, type(uint256).max) — exact amounts only
□ safeApprove(pool, 0) before safeApprove(pool, amount) on each hop
```

### Bot Engine
```
□ Block subscription stable for 24h
□ All 5 filter layers working (verified with live pool states)
□ Algebraic simulation accuracy < 0.1% divergence from eth_call
□ find_optimal_borrow_size() tested on all 4 pool combinations
□ Startup pool config audit logged (A, impact%, fee_source per pool)
□ Fee-rate warnings shown for any pool missing on-chain fee()
□ Aave reserve check fires on every cycle (not just startup)
□ Bot auto-halts when Aave reserve frozen/paused
□ Gas regime transitions tested: normal → alert → critical → halt
□ Consecutive revert counter triggers at 5 error reverts
□ Race losses (ProfitBelowMin) do NOT trip circuit breaker
□ Telegram alerts firing for all defined triggers
□ Structured JSON logs only (no println! in codebase)
□ BOT_PRIVATE_KEY never logged (grep logs for key string)
□ API key never logged
□ Parameter hot-reload tested (change via dashboard, verify in next log)
```

### Dashboard
```
□ TESTNET badge in amber on testnet; MAINNET badge in green on mainnet
□ All metrics populate from real API (no hardcoded values)
□ Kill switch PAUSE and RESUME work end-to-end
□ Parameter page saves all fields correctly
□ Parameter changes survive bot restart
□ WebSocket reconnects automatically after bot restart
□ Mobile layout correct at 375px width
□ History page shows recent transactions
□ Pool states update in real time
```

### Security
```
□ .env.mainnet is in .gitignore and never committed
□ BOT_PRIVATE_KEY only in /etc/kingfisher/kingfisher.env (chmod 600, root-only)
□ API_KEY is a random 32+ character string
□ Bot wallet has ONLY gas ETH
□ Contract owner is cold wallet (Ledger/Trezor)
□ Cold wallet never touches the bot server
□ Server firewall exposes no public port other than the API (3001)
□ Vercel dashboard requires API key to connect
```

### Mainnet Dry Run (Strongly Recommended)
```
□ Deploy to mainnet with MIN_PROFIT_USD=100000 (never fires in practice)
□ Run for 24h — confirms scanning, filtering, simulation work
□ Verify opportunity detection rate matches projections (18–35/day calm)
□ Verify no unexpected errors: journalctl -u kingfisher --since "24h ago" | grep ERROR
□ Set MIN_PROFIT_USD=75 only after dry run passes
```

---

## First 48 Hours on Mainnet

Watch these metrics hourly after going live:

```
□ At least 1 trade fired in first 6 hours (confirms end-to-end works)
□ Win rate > 60%
□ Gas cost per tx < $2 (Arbitrum — if higher, the L1 gas model needs tuning)
□ No reverts with ProfitBelowMin beyond normal race losses (sustained = inaccurate sim)
□ No reverts with NotAavePool() (means security issue — stop immediately)
□ Profit accumulates in contract (check balance periodically)
□ First withdrawal to cold wallet succeeds
□ No process memory leaks (RSS stable — check `systemctl status kingfisher`)
□ WebSocket stays live (dashboard never shows Disconnected)
```
