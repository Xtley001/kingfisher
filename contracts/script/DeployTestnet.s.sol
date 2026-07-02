// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {Script, console2} from "forge-std/Script.sol";
import {KingfisherArb} from "../src/KingfisherArb.sol";

contract DeployTestnet is Script {
    // Aave V3 Pool on Arbitrum Sepolia
    address constant AAVE_POOL = 0xBfC91D59fdAA134A4ED45f7B584cAf96D7792Eff;

    // $1 minimum for testnet testing
    uint256 constant MIN_PROFIT_WEI = 1e6;

    function run() external {
        // Guard against accidental deployment to the wrong testnet.
        // Arbitrum Sepolia chain IDs: 421614 (current) or 421613 (legacy).
        require(
            block.chainid == 421614 || block.chainid == 421613,
            "DeployTestnet.s.sol: must deploy on Arbitrum Sepolia (chain_id 421614 or 421613)"
        );

        uint256 deployerKey = vm.envUint("BOT_PRIVATE_KEY");

        console2.log("Deploying KingfisherArb to Arbitrum SEPOLIA (testnet)");
        console2.log("Deployer:", vm.addr(deployerKey));

        // Sepolia: start with empty pool list — add verified addresses after deploy
        // Check https://curve.fi/#/arbitrum-sepolia/pools for current deployments
        address[] memory initialPools = new address[](0);

        vm.startBroadcast(deployerKey);

        // HIGH-01 fix: 3-arg constructor — operator defaults to deployer (msg.sender)
        KingfisherArb arb = new KingfisherArb(
            AAVE_POOL,
            MIN_PROFIT_WEI,
            initialPools
        );

        console2.log("KingfisherArb TESTNET deployed:", address(arb));
        console2.log("Operator (deployer initially):", arb.operator());
        console2.log("Set in .env.testnet: CONTRACT_ADDRESS_TESTNET=", address(arb));

        vm.stopBroadcast();
    }
}
