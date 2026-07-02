#!/usr/bin/env bash
# ─── gen_access_list.sh ───────────────────────────────────────────────────────
# Generate real EIP-2930 access list slot hashes for each Curve pool.
# Run this ONCE offline and paste the output into submission.rs (set TransactionRequest.access_list) once calibrated.
#
# HIGH-04: The previous implementation used placeholder sequential slots
# [0x00, 0x01, 0x02...] which are NOT Curve Vyper storage slots. This script
# derives real slot hashes from an actual test arb calldata trace.
#
# Prerequisites:
#   - cast (Foundry) in PATH
#   - RPC_HTTP_URL set to an Arbitrum One RPC
#   - CONTRACT_ADDRESS_MAINNET set to the deployed KingfisherArb address
#   - COLD_WALLET_ADDR set to any address with ETH (read-only simulation)
#
# Usage:
#   export RPC_HTTP_URL=https://...
#   export CONTRACT_ADDRESS_MAINNET=0x...
#   export COLD_WALLET_ADDR=0x...
#   bash scripts/gen_access_list.sh
# ─────────────────────────────────────────────────────────────────────────────

set -e

RPC="${RPC_HTTP_URL:?Set RPC_HTTP_URL}"
CONTRACT="${CONTRACT_ADDRESS_MAINNET:?Set CONTRACT_ADDRESS_MAINNET}"
FROM="${COLD_WALLET_ADDR:?Set COLD_WALLET_ADDR}"

FRAX_USDC="0x0c9b8A3FDECb9d5B218D02555a8BaF332e5b740d"
CRVUSD_USDC="0xec090cf6DD891D2d014beA6edAda6e05E025D93d"
CRVUSD_USDT="0x73aF1150F265419Ef8a5DB41908B700C32D49135"
TWOPOOL="0x7f90122BF0700F9E7e1F688fe926940E8839F353"

echo ""
echo "═══════════════════════════════════════════════════════════"
echo "  Kingfisher EIP-2930 Access List Generator"
echo "  Contract : $CONTRACT"
echo "  From     : $FROM"
echo "  RPC      : $RPC"
echo "═══════════════════════════════════════════════════════════"
echo ""

gen_access_list() {
    local label="$1"
    local pool_a="$2"
    local pool_b="$3"
    local i_a="$4"
    local j_a="$5"
    local i_b="$6"
    local j_b="$7"
    local flash_token="$8"
    local flash_amount="$9"

    echo "── $label ──────────────────────────────────────────────"
    cast access-list \
        --rpc-url "$RPC" \
        --from "$FROM" \
        "$CONTRACT" \
        "executeArb(address,uint256,(address,int128,int128,bool,uint256)[],uint256)" \
        "$flash_token" \
        "$flash_amount" \
        "[($pool_a,$i_a,$j_a,false,990000000),($pool_b,$i_b,$j_b,false,990000000)]" \
        "1000000" 2>/dev/null || echo "(reverted — contract may not be deployed or pool paused; slots still printed above the revert)"
    echo ""
}

# native USDC
USDC="0xaf88d065e77c8cC2239327C5EDb3A432268e5831"
USDT="0xFd086bC7CD5C481DCC9C85ebE478A1C0b69FCbb9"

gen_access_list "crvUSD-USDC → 2pool (USDC flash)" \
    "$CRVUSD_USDC" "$TWOPOOL" \
    1 0  0 1 \
    "$USDC" "1000000000"

gen_access_list "crvUSD-USDT → 2pool (USDT flash)" \
    "$CRVUSD_USDT" "$TWOPOOL" \
    1 0  1 0 \
    "$USDT" "1000000000"

gen_access_list "FRAX-USDC → crvUSD-USDC (USDC flash)" \
    "$FRAX_USDC" "$CRVUSD_USDC" \
    1 0  0 1 \
    "$USDC" "1000000000"

echo ""
echo "═══════════════════════════════════════════════════════════"
echo "  Paste the storage_keys arrays above into:"
echo "  bot/crates/executor/src/submission.rs  (apply to TransactionRequest.access_list)"
echo "  Match each address to its slot list."
echo "═══════════════════════════════════════════════════════════"
