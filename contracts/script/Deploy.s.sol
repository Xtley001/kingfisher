// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {Script, console2} from "forge-std/Script.sol";
import {KingfisherArb} from "../src/KingfisherArb.sol";

contract DeployMainnet is Script {
    // Aave V3 Pool on Arbitrum One
    address constant AAVE_POOL = 0x794a61358D6845594F94dc1DB02A252b5b4814aD;

    // $75 USDC minimum profit (USDC has 6 decimals)
    uint256 constant MIN_PROFIT_WEI = 75e6;

    function run() external {
        // Guard against accidental deployment to the wrong chain.
        // The Aave pool address 0x794a61... is Arbitrum One only; deploying elsewhere
        // connects the bot to an unverified contract with no flash loan callback guard.
        require(
            block.chainid == 42161,
            "Deploy.s.sol: must deploy on Arbitrum One (chain_id 42161)"
        );

        // DEPLOY FROM COLD WALLET (BOT_PRIVATE_KEY = cold wallet key during deploy)
        // After deploy, immediately call setOperator(hotWalletAddress) from the cold wallet.
        uint256 deployerKey = vm.envUint("COLD_WALLET_PRIVATE_KEY");
        address deployer = vm.addr(deployerKey);

        // HIGH-01 fix: 3-arg constructor — operator defaults to deployer (cold wallet).
        // Immediately after deploy, call setOperator(HOT_WALLET) from the cold wallet
        // so the bot (hot wallet) can call executeArb(), while only the cold wallet
        // can withdraw profits, rotate operator, and update the allowlist.
        address[] memory initialPools = new address[](4);
        // Correct Curve FRAXBP plain pool on Arbitrum One (0xC9B8a3FDECB9D5b218d02555a8Baf332E5B740d5)
        initialPools[0] = 0xC9B8a3FDECB9D5b218d02555a8Baf332E5B740d5; // FRAX-USDC.e (FRAXBP)
        initialPools[1] = 0xec090cf6DD891D2d014beA6edAda6e05E025D93d; // crvUSD-USDC
        initialPools[2] = 0x73aF1150F265419Ef8a5DB41908B700C32D49135; // crvUSD-USDT
        initialPools[3] = 0x7f90122BF0700F9E7e1F688fe926940E8839F353; // 2pool

        console2.log("Deploying KingfisherArb to Arbitrum One");
        console2.log("Deployer (cold wallet):", deployer);

        vm.startBroadcast(deployerKey);

        address constant BALANCER_VAULT = 0xBA12222222228d8Ba445958a75a0704d566BF2C8;

        // 4-arg constructor: operator = msg.sender (cold wallet) initially
        KingfisherArb arb = new KingfisherArb(
            AAVE_POOL,
            BALANCER_VAULT,
            MIN_PROFIT_WEI,
            initialPools
        );

        console2.log("KingfisherArb deployed:", address(arb));
        console2.log("Owner (cold wallet):", arb.owner());
        console2.log("Operator (initially cold wallet):", arb.operator());
        console2.log("NEXT STEP: call setOperator(HOT_WALLET) from cold wallet:");
        console2.log("  cast send", address(arb), "\"setOperator(address)\" $HOT_WALLET_ADDRESS \\");
        console2.log("    --private-key $COLD_WALLET_PRIVATE_KEY --rpc-url $RPC_HTTP_URL");
        console2.log("Set in .env.mainnet: CONTRACT_ADDRESS_MAINNET=", address(arb));

        vm.stopBroadcast();
    }
}
