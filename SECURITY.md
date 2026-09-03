# Security Policy

_Security model, vulnerability disclosure process, and on-chain verification mechanisms for Kingfisher._

## Supported Versions

Kingfisher is pre-production software. Only the latest commit on the `main` branch receives active maintenance and security patches.

| Version | Supported | Notes |
|---|---|---|
| `main` | Yes | Active development and bug fixes |
| Tagged releases | No | Pre-release staging |

## Reporting a Vulnerability

If you identify a security vulnerability or exploit vector, disclose it responsibly. Do **not** open public GitHub issues for vulnerabilities that could compromise funds or operator keys.

- Submit a private report via [GitHub Security Advisories](https://github.com/Xtley001/kingfisher/security/advisories/new), or
- Contact the maintainers directly via the email listed on the repository profile.

Include detailed reproduction steps and, where feasible, a minimal Foundry test case or proof-of-concept. Acknowledgments are issued within 72 hours.

## Invariants & Capital Safety

Kingfisher executes strictly uncollateralized flash-loan arbitrage. Capital safety is guaranteed through protocol-level constraints:

1. **Zero Deposit Custody**: The contract holds no user deposits or pooled liquidity. Profits accumulate in the execution contract until swept to a cold multi-sig or hardware wallet.
2. **Atomic Execution**: Borrowing, multi-hop swapping, fee calculation, and profit validation occur within a single atomic Ethereum transaction.
3. **Strict Profit Assertion**: The contract checks `require(netProfit >= minProfit, ProfitBelowMin())`. If market conditions shift or competing transactions alter pool balances, the transaction reverts completely, expending only transaction gas fees.

## On-Chain Controls

| Control | Implementation | Purpose |
|---|---|---|
| **Balancer Callback Auth** | `receiveFlashLoan()` in `KingfisherArb.sol` | Enforces `caller == address(BALANCER_VAULT)`, reverting with `NotBalancerVault()`. |
| **Aave Callback Auth** | `executeOperation()` in `KingfisherArb.sol` | Enforces `caller == address(AAVE_POOL)` and `initiator == address(this)`, reverting with `NotAavePool()` / `BadInitiator()`. |
| **Role Separation** | `Ownable2Step` owner vs `operator` | Hot operator key can only invoke `executeArb*`; cold owner controls withdrawals, allowlists, and operator rotation. |
| **Reentrancy Lock** | EIP-1153 transient storage (`tstore` / `tload`) | Blocks cross-function or reentrant re-invocation during callback processing. |
| **Pool Allowlist** | `allowedPools` mapping | Prevents execution through arbitrary or untrusted Curve pool implementations. |
| **Dynamic Slippage Floor** | `Hop.minAmountOut > 0` | Rejects transactions where intermediate swaps incur excessive price impact. |
| **Exact-Amount Approvals** | Inline Yul `_approve()` with zero-reset | Avoids `type(uint256).max` infinite allowances and mitigates USDT approval race conditions. |
| **Pool Sanity Gate** | `get_virtual_price() >= 1e18` | Sanity floor rejecting severely drained or structurally compromised pools. |

## Trust Assumptions

1. **Arbitrum Sequencer Integrity**: Assumes standard Nitro sequencer operation. While frontrunning is structurally prevented by the absence of a public mempool, sequencer outages or reorderings could affect latency-sensitive transactions.
2. **Lending Protocol Solvency**: Relies on Balancer V2 Vault and Aave V3 Pool contract integrity on Arbitrum One. If Aave reserve status freezes, the bot's health monitor triggers an automated shutdown.
3. **Curve Invariant Stability**: Curve pools are verified against on-chain code before allowlisting.

## Operational Security

- **Key Isolation**: `BOT_PRIVATE_KEY` resides only in `/etc/kingfisher/kingfisher.env` (`chmod 600`, root access only). Keys are format-validated before ingestion to prevent leakage into stack traces.
- **Wallet Segregation**: The bot operator hot wallet holds only gas ETH (0.3–1.0 ETH). Profits accumulate in `KingfisherArb` and are swept to a cold Ledger or Gnosis Safe.
- **API Surface**: The management API requires a static `X-Api-Key` header over TLS. `/metrics` endpoints must be restricted behind local reverse proxies or private VPCs.
