# Security Policy

## Reporting a Vulnerability

If you discover a security issue in Kingfisher, please report it privately. Do **not**
open a public issue for anything that could put funds at risk.

- Open a [GitHub security advisory](https://github.com/Xtley001/kingfisher/security/advisories/new), or
- Email the maintainer (see the GitHub profile for contact).

Please include reproduction steps and, where possible, a proof-of-concept. We aim to
acknowledge reports within 72 hours.

---

## Design: Why Funds Are Bounded

Kingfisher runs atomic flash-loan arbitrage. Every trade is a single transaction that:

1. borrows from Aave V3,
2. executes a Curve swap route, and
3. is guarded on-chain by `require(netProfit ≥ minProfit)` in
   `KingfisherArb.executeOperation()`.

If the trade would not clear the profit floor, the entire transaction reverts. A losing
race or a mispriced simulation therefore costs **only gas** — never principal. The bot
holds no user deposits; profit accrues inside the contract and is swept to a cold wallet.

---

## On-Chain Controls

| Control | Where | Purpose |
|---|---|---|
| Owner / operator separation | `operator` vs `Ownable2Step` owner | Hot wallet only calls `executeArb`; cold wallet owns funds and can rotate the operator if the hot key leaks |
| Aave callback authentication | `executeOperation` | Requires `caller == AAVE_POOL` **and** `initiator == address(this)` |
| Pool allowlist | `allowedPools` | Only pre-approved Curve pools can be routed |
| Reentrancy lock | EIP-1153 transient storage | Blocks reentrant `executeArb` / `executeOperation` |
| Atomic profit guard | `ProfitBelowMin` custom error | Reverts unprofitable trades |
| Per-hop min output | `Hop.minAmountOut > 0` | Rejects silent-slippage routes |
| Exact-amount approvals | assembly `_approve` with zero-reset | No `approve(MAX_UINT)`; USDT-safe |
| Pool health check | `get_virtual_price() ≥ 1e18` | Sanity floor before routing (see limits below) |

---

## Trust Assumptions

Kingfisher's safety depends on the following. A reviewer should weigh each:

- **Arbitrum sequencer** — centralized (Offchain Labs), first-come-first-served with
  Timeboost priority auctions. Kingfisher assumes honest sequencing; it does not defend
  against sequencer misbehavior (no protocol on Arbitrum currently does).
- **Aave V3 solvency and reserve status** — the bot halts automatically if the USDC
  reserve is frozen/paused.
- **Curve pool integrity** — a compromised or manipulated pool could pass the
  `get_virtual_price() ≥ 1e18` check. This check is a sanity floor, not a manipulation
  defense. New pools require manual, multi-step approval (see docs/OPS_MANUAL.md).
- **Chainlink price feeds** — used for peg-stress detection and ETH pricing; a stale or
  manipulated feed degrades opportunity detection but cannot bypass the on-chain profit
  guard.

---

## Operational Security

- `BOT_PRIVATE_KEY` and RPC URLs live only in `/etc/kingfisher/kingfisher.env`
  (`chmod 600`, root-readable) — never in the repository. The bot never logs the key
  (keys are format-validated before parsing so they cannot leak into an error chain).
- The hot (operator) wallet holds only gas ETH.
- The cold (owner) wallet is a hardware wallet or Gnosis Safe and never touches the bot
  server.
- The API is protected by a static `X-Api-Key`; the server firewall must not expose any
  port other than the API. `/metrics` is unauthenticated by design — keep it behind the
  firewall.

---

## Known Limitations / Pre-Mainnet TODO

- **Audit pending.** The contract uses hand-written Yul in `_approve` and the caller
  checks. These are correct on review but should receive an independent audit before
  significant capital is deployed. This is tracked as a launch-blocker for large size.
- The L1 gas cost model is an estimate tuned by `L1_BASE_FEE_GWEI`; it is cross-checked
  by the `eth_call` validator but is not a live `ArbGasInfo` read (a planned improvement).
- Run `slither` on the contracts as part of the review process.
