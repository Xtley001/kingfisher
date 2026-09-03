# Contributing

_Guidelines for contributors, development environment configuration, and pull request workflows for Kingfisher._

## Development Environment

| Component | Toolchain | Required Version |
|---|---|---|
| Bot Engine | Rust, Cargo | 1.80+ |
| Smart Contracts | Foundry (`forge`, `cast`) | Latest stable (`foundryup`) |
| Operator Dashboard | Node.js, npm | Node 20+, npm 10+ |

### Setup

```bash
# Bot engine
cd bot && cargo build

# Smart contracts
cd ../contracts
forge install foundry-rs/forge-std --no-git
forge install aave/aave-v3-core --no-git
forge install OpenZeppelin/openzeppelin-contracts --no-git
forge build

# Operator dashboard
cd ../dashboard
npm install
npm run dev
```

## Pre-PR Validation

Before submitting a pull request, verify that all validation steps pass cleanly:

```bash
# Rust engine formatting, lints, and test suite
cd bot
cargo fmt --check
cargo clippy -- -D warnings
cargo test --all

# Solidity contract formatting, build, and unit tests
cd ../contracts
forge fmt --check
forge build
forge test
```

## Adding a Curve Pool

Every newly added pool must undergo on-chain verification before inclusion in `bot/crates/core/src/venues/arbitrum.rs`:

1. **Verify Pool Contract**: Confirm the pool address on Arbiscan.
2. **Query Coin Indices**: Fetch token addresses via `cast call <pool> "coins(uint256)(address)" 0/1`.
3. **Query Amplification Factor**: Confirm the amplification parameter via `cast call <pool> "A()(uint256)"` — never assume or hardcode default values.
4. **Allowlist on Contract**: Execute `setPoolAllowed(address, true)` via the cold owner wallet.

## Code Style

- **Rust**: Follow standard `cargo fmt` conventions. Zero clippy warnings permitted. Avoid `.unwrap()` or `.expect()` in runtime execution paths; use `?` or explicit error handling with custom errors.
- **Solidity**: Follow Foundry default formatting (`forge fmt`). Use custom errors (`error Name()`) instead of string reverts (`require(cond, "string")`). Document assembly blocks with explicit memory and transient storage layouts.
- **Comments**: Keep comments focused on intent, invariant constraints, and non-obvious math rather than reiterating function names or recording git change histories.

## Pull Request Guidelines

- **Atomic Scopes**: Submit focused, single-purpose pull requests.
- **Test Coverage**: Accompany any behavioral modification with corresponding Rust unit tests and Foundry fork tests.
- **Execution Path Changes**: Any modifications affecting sizing, routing, or transaction construction must document fork simulation results.

## Commit Conventions

Use conventional commit messages with imperative, present-tense summaries:

- `feat:` — New capabilities or venue integrations
- `fix:` — Bug fixes or edge condition corrections
- `test:` — Test suite expansions and fork test harnesses
- `docs:` — Documentation and architectural spec updates
- `chore:` — Dependencies, tooling, and build configurations
