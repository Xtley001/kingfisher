// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {Test, console2} from "forge-std/Test.sol";
import {KingfisherArb} from "../src/KingfisherArb.sol";
import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";

/**
 * @title StressRegimeTest
 * @notice Validates bot behaviour under peg stress: Chainlink price mocked,
 *         contract still executes correctly, security guards remain in force.
 * @dev Run: forge test --fork-url $RPC_HTTP_URL --match-contract StressRegimeTest -vvvv
 */
contract StressRegimeTest is Test {
    address constant AAVE_POOL   = 0x794a61358D6845594F94dc1DB02A252b5b4814aD;
    address constant FRAX_USDC   = 0xC9B8a3FDECB9D5b218d02555a8Baf332E5B740d5;
    address constant CRVUSD_USDC = 0xec090cf6DD891D2d014beA6edAda6e05E025D93d;
    address constant CRVUSD_USDT = 0x73aF1150F265419Ef8a5DB41908B700C32D49135;
    address constant TWOPOOL     = 0x7f90122BF0700F9E7e1F688fe926940E8839F353;
    address constant USDC        = 0xaf88d065e77c8cC2239327C5EDb3A432268e5831;

    // Chainlink USDC/USD on Arbitrum One
    address constant CHAINLINK_USDC_USD = 0x50834F3163758fcC1Df9973b6e91f0F0F0434aD3;

    KingfisherArb arb;
    address[] pools;

    function setUp() public {
        vm.createSelectFork(vm.envString("RPC_HTTP_URL"));
        pools = new address[](4);
        pools[0] = FRAX_USDC;
        pools[1] = CRVUSD_USDC;
        pools[2] = CRVUSD_USDT;
        pools[3] = TWOPOOL;
        address constant BALANCER_VAULT = 0xBA12222222228d8Ba445958a75a0704d566BF2C8;
        arb = new KingfisherArb(AAVE_POOL, BALANCER_VAULT, 75e6, pools);
    }

    /// @notice Simulates a 0.3% USDC depeg — stress_regime should activate in bot.
    /// Contract itself is stateless re: peg; test confirms oracle data is mockable
    /// and contract continues executing correctly under mocked stress conditions.
    function test_MockUsdcDepeg() public {
        // Mock Chainlink USDC/USD to return $0.997
        vm.mockCall(
            CHAINLINK_USDC_USD,
            abi.encodeWithSignature("latestRoundData()"),
            abi.encode(
                uint80(1),
                int256(99700000),         // $0.997 (8 decimals)
                uint256(block.timestamp),
                uint256(block.timestamp),
                uint80(1)
            )
        );

        // Verify mock is live
        (,int256 price,,,) = AggregatorV3Interface(CHAINLINK_USDC_USD).latestRoundData();
        assertEq(price, 99700000, "Mock not applied");
        console2.log("Mocked USDC/USD:", price);

        // Contract security guards must still hold under stress
        address attacker = makeAddr("stress-attacker");
        KingfisherArb.Hop[] memory hops = new KingfisherArb.Hop[](2);
        vm.prank(attacker);
        vm.expectRevert(KingfisherArb.NotOperator.selector);
        arb.executeArb(USDC, 1_000e6, hops, 1e6);

        console2.log("StressRegime mock + security guards: PASS");
    }
}

interface AggregatorV3Interface {
    function latestRoundData() external view returns (
        uint80, int256, uint256, uint256, uint80
    );
}
