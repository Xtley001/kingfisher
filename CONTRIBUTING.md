# Contributing to Kingfisher

## Setup

```bash
# Rust bot
cd bot && cargo build

# Contracts
cd contracts
forge install foundry-rs/forge-std --no-git
forge install aave/aave-v3-core --no-git
forge install OpenZeppelin/openzeppelin-contracts --no-git
forge build

# Dashboard
cd dashboard && npm install && npm run dev
```

## Before Opening a PR

```bash
cd bot && cargo test --all         # must pass
cd bot && cargo clippy             # zero warnings
cd contracts && forge build        # must compile
cd contracts && forge test         # unit tests only (no RPC required)
```

## Adding a Pool

See `docs/OPS_MANUAL.md` for the full pool approval process and the `PoolConfig` template
in `bot/crates/core/src/config.rs`.

Verify all three before submitting:
1. Pool address confirmed on Arbiscan
2. Token addresses confirmed via `pool.coins(0)` and `pool.coins(1)` on-chain
3. A-parameter confirmed via `pool.A()` on-chain — never guess

```bash
# Verify pool tokens on-chain
cast call $POOL_ADDRESS "coins(uint256)(address)" 0 --rpc-url $RPC_HTTP_URL
cast call $POOL_ADDRESS "coins(uint256)(address)" 1 --rpc-url $RPC_HTTP_URL
cast call $POOL_ADDRESS "A()(uint256)" --rpc-url $RPC_HTTP_URL
```

## Code Style

- Rust: `cargo fmt` before committing. Clippy warnings are treated as errors in CI.
- Solidity: Foundry default formatting. Custom errors only (no `require(false, "string")`).
- No `unwrap()` in production paths — use `?` or handle errors explicitly.

## Commit Messages

Follow conventional commits:
- `fix:` — bug fix
- `feat:` — new feature
- `test:` — adding or updating tests
- `docs:` — documentation only
- `chore:` — tooling, deps, config

## Questions

Open a GitHub Issue or post in the Arbitrum Discord `#developer-chat`.
