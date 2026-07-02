# Kingfisher — Launch Roadmap

> Sequenced checklist from bare metal to mainnet. Complete each phase before advancing.

---

## Phase 1 — Infrastructure

### 1.1 Server Setup
- Provision a bare metal or dedicated server co-located near Arbitrum sequencer infrastructure
- Ubuntu 22.04 LTS recommended
- Install Rust toolchain: `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
- Install Foundry: `curl -L https://foundry.paradigm.xyz | bash && foundryup`

### 1.2 Arbitrum Nitro Node (IPC Path)
Running a local Nitro node unlocks IPC connectivity (~0.1ms vs 50–300ms for external RPC)
and full local mempool access for the Chainlink mempool monitor.

```bash
# Clone and configure Nitro
git clone https://github.com/OffchainLabs/nitro.git
cd nitro
# Follow official docs: https://docs.arbitrum.io/run-arbitrum-node/overview
# Configure --ipc-path=/tmp/arb-nitro.ipc
```

Set in `.env`:
```
RPC_IPC_PATH=/tmp/arb-nitro.ipc
```

> Before enabling in production: confirm `RPC_IPC_PATH` is set and the IPC socket exists on the server.
> Test locally with `cargo run --features ipc` against a local Nitro node first.
> Without IPC, the Chainlink mempool monitor falls back to HTTP polling.

### 1.3 External RPC Fallback
```
RPC_WS_URL=wss://arb-mainnet.g.alchemy.com/v2/YOUR_KEY
RPC_HTTP_URL=https://arb-mainnet.g.alchemy.com/v2/YOUR_KEY
RPC_FALLBACK_URL=wss://arbitrum-one.quiknode.pro/YOUR_KEY
```

---

## Phase 2 — Testnet Validation

### 2.1 Deploy to Arbitrum Sepolia
```bash
cd contracts
cp ../.env.example ../.env
# Fill: COLD_WALLET_PRIVATE_KEY, RPC_HTTP_URL (Sepolia), NETWORK=testnet
source ../.env
forge script script/DeployTestnet.s.sol --rpc-url $RPC_HTTP_URL --broadcast --verify
```

Save the deployed address to `CONTRACT_ADDRESS_TESTNET` in `.env`.

### 2.2 Verify Aave Flash Loan Access
```bash
# Check Aave maxFlashLoan(USDC) returns non-zero on Arbitrum Sepolia
cast call 0xBfC91D59fdAA134A4ED45f7B584cAf96D7792Eff \
  "maxFlashLoan(address)(uint256)" \
  0x75faf114eafb1BDbe2F0316DF893fd58CE46AA4d \
  --rpc-url $RPC_HTTP_URL
```
Expected: non-zero value (Aave USDC liquidity available).

### 2.3 Run Fork Tests
```bash
cd contracts
forge test --fork-url $RPC_HTTP_URL --match-test test_RealArb -vvvv
forge test --fork-url $RPC_HTTP_URL --match-contract StressRegimeTest -vvvv
```

### 2.4 Run Bot on Testnet
```bash
cd bot
NETWORK=testnet cargo run --features ipc
```

Confirm in logs:
- `🟡 Network: TESTNET` on startup
- `Token alignment check complete` with no warnings
- Block loop running, scanner processing pools

---

## Phase 3 — Mainnet Deployment

### 3.1 Pre-Deploy Checklist
- [ ] All fork tests pass clean
- [ ] `cargo build --release --features ipc` compiles with zero warnings
- [ ] Cold wallet is a hardware wallet (Ledger) or Gnosis Safe
- [ ] Hot wallet funded with 0.5–1.0 ETH for gas
- [ ] `COLD_WALLET_PRIVATE_KEY` accessible only for deploy — remove from server immediately after

### 3.2 Deploy Contract
```bash
cd contracts
# NETWORK=mainnet in .env
forge script script/Deploy.s.sol \
  --rpc-url $RPC_HTTP_URL \
  --broadcast \
  --verify \
  --etherscan-api-key $ARBISCAN_KEY
```

Immediately after deploy:
```bash
# Set hot wallet as operator (cold wallet remains owner)
cast send $CONTRACT_ADDRESS_MAINNET \
  "setOperator(address)" \
  $HOT_WALLET_ADDRESS \
  --ledger \
  --rpc-url $RPC_HTTP_URL
```

Remove `COLD_WALLET_PRIVATE_KEY` from server.

### 3.3 Environment Variables (Production)

| Variable | Description |
|---|---|
| `NETWORK` | `mainnet` |
| `RPC_IPC_PATH` | Path to local Nitro IPC socket |
| `RPC_WS_URL` | Arbitrum WebSocket (Alchemy / QuickNode) |
| `RPC_HTTP_URL` | Arbitrum HTTP RPC |
| `RPC_FALLBACK_URL` | Backup WebSocket — different provider |
| `BOT_PRIVATE_KEY` | Hot wallet private key (signs executeArb) |
| `COLD_WALLET_ADDR` | Cold wallet address (receives profits) |
| `TIMEBOOST_EXPRESS_LANE_URL` | Optional — Arbitrum Timeboost express-lane endpoint for priority sequencing |
| `SEQUENCER_BACKUP_URL` | Optional — secondary endpoint for best-effort mirror broadcast |
| `L1_BASE_FEE_GWEI` | Ethereum L1 base fee estimate for the Arbitrum L1 data-fee cost model (default: 10) |
| `ARBISCAN_KEY` | Contract verification |
| `CONTRACT_ADDRESS_MAINNET` | Deployed KingfisherArb address |
| `MIN_PROFIT_USD` | Absolute profit floor (default: 10) |
| `MIN_GAS_ROI` | Min ROI multiple on gas (default: 3.0) |
| `ABS_CAP_USD` | Max flash loan size (default: 5000000) |
| `GAS_LIMIT_OVERRIDE` | Gas limit per tx (default: 750000) |
| `KINGFISHER_DATA_DIR` | Persistent state dir (default: /var/lib/kingfisher) |
| `API_KEY` | Dashboard authentication key |
| `TELEGRAM_BOT_TOKEN` | Telegram alert bot |
| `TELEGRAM_CHAT_ID` | Telegram chat for alerts |

### 3.4 Go/No-Go Checklist
- [ ] `forge test --fork-url $RPC_HTTP_URL` — all tests pass
- [ ] `isPoolHealthy()` returns true for all 4 pools
- [ ] Aave `maxFlashLoan(USDC)` > $1M on mainnet
- [ ] Hot wallet ETH > 0.5 ETH
- [ ] Dashboard showing live block numbers
- [ ] Telegram alerts firing on test message
- [ ] `COLD_WALLET_PRIVATE_KEY` removed from server

---

## Phase 4 — Grant Applications

> Primary angle: **Aave**. Kingfisher is a genuine, measurable flash-loan consumer that
> generates protocol revenue for Aave LPs on every trade — the strongest, most concrete
> pitch. Lead with it. See docs/STRATEGY.md for the full grant framing.

### 4.1 Aave Grants DAO  ⭐ primary
**URL:** https://aavegrants.org

Positioning: Kingfisher is a flash-loan consumer on Aave V3 Arbitrum. Every arb trade
borrows from Aave, pays the 5 bps flash-loan premium, and returns funds atomically. More
arb activity = more flash-loan fee revenue for Aave LPs.

Frame as: "an open-source flash-loan consumer generating protocol revenue on Aave V3,"
and include the on-chain `ArbExecuted` event count + cumulative Aave premium paid as
hard evidence once live.

### 4.2 Arbitrum Foundation
**URL:** https://grants.arbitrum.io

Positioning: MEV tooling that brings arbitrage capital to Arbitrum Curve pools, improving
price efficiency and reducing slippage for all users. Key metrics: pool coverage, daily
trade frequency, volume (post-mainnet).

### 4.3 Alchemy / infra grants
Positioning: open-source Arbitrum MEV tooling; IPC vs WebSocket latency benchmarks as
technical evidence.

### 4.4 Tenderly OSS
**URL:** https://tenderly.co/open-source

For free simulation and monitoring credits. Include the `ARCHITECTURE.md` system diagram
in the application.

---

## Phase 5 — Post-Launch

### 5.1 First 30 Days
- Collect real latency numbers for `PERFORMANCE.md` (IPC vs WS, block pipeline p99)
- Monitor `divergence_pct` in validation logs — should stay < 0.1%
- Track gas usage per route — update `PERFORMANCE.md` gas table
- Adjust `ABS_CAP_USD` upward as P&L data accumulates

### 5.2 Pool Expansion
Add new pools via `PoolConfig` in `bot/crates/core/src/config.rs`.
Follow the pool approval process in `docs/OPS_MANUAL.md` — all three verifications required:
1. Pool address confirmed on Arbiscan
2. Token addresses confirmed via `pool.coins()` on-chain
3. A-parameter confirmed via `pool.A()` on-chain

### 5.3 Profit Withdrawal
Withdraw to cold wallet at minimum weekly. Use Method B in `docs/OPS_MANUAL.md`
(direct `cast send` from cold wallet — no dashboard route exists).
