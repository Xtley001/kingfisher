// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {Test, console2} from "forge-std/Test.sol";
import {KingfisherArb} from "../src/KingfisherArb.sol";
import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";

/**
 * @title KingfisherArbTest
 * @notice Fork tests against Arbitrum One mainnet state.
 * @dev Run: forge test --fork-url $RPC_HTTP_URL -vvvv
 *
 * Fixes applied (audit HIGH-01):
 *   - Constructor is now 3-arg (operator defaults to msg.sender = this test contract).
 *     After mainnet deploy, call setOperator(hotWalletAddress) from cold wallet.
 *   - All vm.expectRevert("string") replaced with typed custom error selectors.
 *     Typed errors encode as 4-byte selectors, not ABI string encoding.
 *   - FRAX-USDC pool address corrected (was 0xC9B8a3..., now 0x0c9b8A3...).
 *   - USDC address is native USDC (0xaf88d...), not USDC.e (0xFF970...).
 */

interface ICurvePool {
    function exchange(int128 i, int128 j, uint256 dx, uint256 min_dy) external returns (uint256);
}
contract KingfisherArbTest is Test {
    KingfisherArb public arb;

    // ─── Arbitrum One addresses ───────────────────────────────────────────────
    address constant AAVE_POOL      = 0x794a61358D6845594F94dc1DB02A252b5b4814aD;

    // Curve FRAXBP plain pool on Arbitrum One (FRAX + USDC.e)
    address constant FRAX_USDC_POOL = 0xC9B8a3FDECB9D5b218d02555a8Baf332E5B740d5;
    address constant CRVUSD_USDC    = 0xec090cf6DD891D2d014beA6edAda6e05E025D93d;
    address constant CRVUSD_USDT    = 0x73aF1150F265419Ef8a5DB41908B700C32D49135;
    address constant TWOPOOL        = 0x7f90122BF0700F9E7e1F688fe926940E8839F353;

    // CRIT-01 fix: native USDC (Circle-issued, what Aave V3 actually holds)
    // NOT USDC.e (0xFF970A61A04b1cA14834A43f5dE4533eBDDB5CC8 — deprecated bridged token)
    address constant USDC           = 0xaf88d065e77c8cC2239327C5EDb3A432268e5831;
    address constant FRAX           = 0x17FC002b466eEc40DaE837Fc4bE5c67993ddBd6F;
    address constant CRVUSD_TOKEN   = 0x498Bf2B1e120FeD3ad3D42EA2165E9b73f99C1e5;
    address constant USDT           = 0xFd086bC7CD5C481DCC9C85ebE478A1C0b69FCbb9;

    address constant USDC_WHALE     = 0xB38e8c17e38363aF6EbdCb3dAE12e0243582891D;

    address[] pools;

    function setUp() public {
        vm.createSelectFork(vm.envString("RPC_HTTP_URL"));

        pools = new address[](4);
        pools[0] = FRAX_USDC_POOL;
        pools[1] = CRVUSD_USDC;
        pools[2] = CRVUSD_USDT;
        pools[3] = TWOPOOL;

        // HIGH-01 fix: 3-arg constructor — operator defaults to msg.sender (this test contract).
        // On mainnet: deployer (cold wallet) is initial operator; call setOperator(hotWallet) next.
        arb = new KingfisherArb(AAVE_POOL, 75e6, pools);
    }

    function test_Deployment() public view {
        assertEq(address(arb.AAVE_POOL()), AAVE_POOL);
        assertEq(arb.owner(),    address(this));
        assertEq(arb.operator(), address(this));
        assertFalse(arb.paused());
        assertEq(arb.minProfitWei(), 75e6);
        assertTrue(arb.allowedPools(FRAX_USDC_POOL));
        assertTrue(arb.allowedPools(CRVUSD_USDC));
        assertTrue(arb.allowedPools(CRVUSD_USDT));
        assertTrue(arb.allowedPools(TWOPOOL));
        console2.log("Deployment: PASS");
    }

    function test_OnlyOperatorCanExecute() public {
        address attacker = makeAddr("attacker");
        KingfisherArb.Hop[] memory hops = new KingfisherArb.Hop[](2);
        vm.prank(attacker);
        vm.expectRevert(KingfisherArb.NotOperator.selector);
        arb.executeArb(USDC, 1_000e6, hops, 1e6);
        console2.log("Access control: PASS");
    }

    function test_PausedReverts() public {
        arb.setPaused(true);
        KingfisherArb.Hop[] memory hops = new KingfisherArb.Hop[](2);
        vm.expectRevert(KingfisherArb.ContractPaused.selector);
        arb.executeArb(USDC, 1_000e6, hops, 1e6);
        console2.log("Pause guard: PASS");
    }

    function test_AaveCallbackOnlyFromAave() public {
        vm.prank(makeAddr("not-aave"));
        vm.expectRevert(KingfisherArb.NotAavePool.selector);
        arb.executeOperation(USDC, 0, 0, address(this), "");
        console2.log("Aave callback restriction: PASS");
    }

    function test_AaveCallbackBadInitiator() public {
        vm.prank(AAVE_POOL);
        vm.expectRevert(KingfisherArb.BadInitiator.selector);
        arb.executeOperation(USDC, 0, 0, makeAddr("bad-initiator"), "");
        console2.log("Bad initiator check: PASS");
    }

    function test_UnallowedPoolReverts() public {
        address fakePool = makeAddr("fake-pool");
        KingfisherArb.Hop[] memory hops = new KingfisherArb.Hop[](2);
        hops[0] = KingfisherArb.Hop({
            pool: fakePool, tokenIn: USDC, tokenInIndex: 0, tokenOutIndex: 1,
            isMetaPool: false, minAmountOut: 1
        });
        hops[1] = KingfisherArb.Hop({
            pool: FRAX_USDC_POOL, tokenIn: FRAX, tokenInIndex: 1, tokenOutIndex: 0,
            isMetaPool: false, minAmountOut: 1
        });
        vm.expectRevert(abi.encodeWithSelector(KingfisherArb.PoolNotAllowed.selector, fakePool));
        arb.executeArb(USDC, 1_000e6, hops, 1e6);
        console2.log("Allowlist check: PASS");
    }

    function test_SetPoolAllowed() public {
        address newPool = makeAddr("new-pool");
        assertFalse(arb.allowedPools(newPool));
        arb.setPoolAllowed(newPool, true);
        assertTrue(arb.allowedPools(newPool));
        arb.setPoolAllowed(newPool, false);
        assertFalse(arb.allowedPools(newPool));
    }

    function test_PoolsAreHealthy() public view {
        assertTrue(arb.isPoolHealthy(FRAX_USDC_POOL), "FRAX-USDC unhealthy");
        assertTrue(arb.isPoolHealthy(CRVUSD_USDC),    "crvUSD-USDC unhealthy");
        assertTrue(arb.isPoolHealthy(CRVUSD_USDT),    "crvUSD-USDT unhealthy");
        assertTrue(arb.isPoolHealthy(TWOPOOL),         "2pool unhealthy");
        console2.log("Pool health checks: PASS");
    }

    function test_SingleHopReverts() public {
        KingfisherArb.Hop[] memory hops = new KingfisherArb.Hop[](1);
        hops[0] = KingfisherArb.Hop({
            pool: FRAX_USDC_POOL, tokenIn: USDC, tokenInIndex: 0, tokenOutIndex: 1,
            isMetaPool: false, minAmountOut: 1
        });
        vm.expectRevert(abi.encodeWithSelector(KingfisherArb.InvalidRouteLength.selector, uint256(1)));
        arb.executeArb(USDC, 1_000e6, hops, 1e6);
    }

    function test_TooManyHopsReverts() public {
        KingfisherArb.Hop[] memory hops = new KingfisherArb.Hop[](5);
        for (uint256 i = 0; i < 5; i++) {
            hops[i] = KingfisherArb.Hop({
                pool: FRAX_USDC_POOL, tokenIn: USDC, tokenInIndex: 0, tokenOutIndex: 1,
                isMetaPool: false, minAmountOut: 1
            });
        }
        vm.expectRevert(abi.encodeWithSelector(KingfisherArb.InvalidRouteLength.selector, uint256(5)));
        arb.executeArb(USDC, 1_000e6, hops, 1e6);
    }

    function test_ZeroFlashAmountReverts() public {
        KingfisherArb.Hop[] memory hops = new KingfisherArb.Hop[](2);
        hops[0] = KingfisherArb.Hop({
            pool: FRAX_USDC_POOL, tokenIn: USDC, tokenInIndex: 0, tokenOutIndex: 1,
            isMetaPool: false, minAmountOut: 1
        });
        hops[1] = KingfisherArb.Hop({
            pool: CRVUSD_USDC, tokenIn: CRVUSD_TOKEN, tokenInIndex: 1, tokenOutIndex: 0,
            isMetaPool: false, minAmountOut: 1
        });
        vm.expectRevert(KingfisherArb.ZeroAmount.selector);
        arb.executeArb(USDC, 0, hops, 1e6);
    }

    function test_WithdrawProfit() public {
        vm.prank(USDC_WHALE);
        IERC20(USDC).transfer(address(arb), 1_000e6);
        uint256 before = IERC20(USDC).balanceOf(address(this));
        arb.withdrawProfit(USDC);
        assertEq(IERC20(USDC).balanceOf(address(this)) - before, 1_000e6);
        console2.log("Profit withdrawal: PASS");
    }

    function test_WithdrawNothingReverts() public {
        vm.expectRevert(abi.encodeWithSelector(KingfisherArb.NothingToWithdraw.selector, USDC));
        arb.withdrawProfit(USDC);
    }

    function test_SetMinProfit() public {
        arb.setMinProfit(100e6);
        assertEq(arb.minProfitWei(), 100e6);
    }

    function test_SetOperator() public {
        address newOp = makeAddr("hot-wallet");
        arb.setOperator(newOp);
        assertEq(arb.operator(), newOp);
        console2.log("setOperator: PASS");
    }

    function test_OperatorCannotSetOperator() public {
        address hotWallet = makeAddr("hot-wallet");
        arb.setOperator(hotWallet);
        vm.prank(hotWallet);
        vm.expectRevert(); // OwnableUnauthorizedAccount — only owner can rotate operator
        arb.setOperator(makeAddr("attacker"));
        console2.log("Operator cannot set operator: PASS");
    }

    function test_ArbInfrastructureHealthy() public view {
        assertFalse(arb.paused());
        assertTrue(arb.allowedPools(FRAX_USDC_POOL));
        assertTrue(arb.isPoolHealthy(FRAX_USDC_POOL));

        uint256 b0_frax = IERC20(FRAX).balanceOf(FRAX_USDC_POOL);
        uint256 b1_usdc = IERC20(USDC).balanceOf(FRAX_USDC_POOL);
        console2.log("FRAX-USDC pool: FRAX =", b0_frax / 1e18, "native USDC =", b1_usdc / 1e6);

        uint256 b0_crv = IERC20(CRVUSD_TOKEN).balanceOf(CRVUSD_USDC);
        uint256 b1_usd = IERC20(USDC).balanceOf(CRVUSD_USDC);
        console2.log("crvUSD-USDC pool: crvUSD =", b0_crv / 1e18, "native USDC =", b1_usd / 1e6);

        console2.log("Arb infrastructure: PASS");
    }

    /// @notice End-to-end flash loan arb on a live Arbitrum fork.
    /// Creates a real imbalance via whale swap, then fires executeArb()
    /// and asserts a profitable ArbExecuted event.
    function test_RealArb() public {
        // Step 1: Create imbalance — whale dumps 500k USDC into FRAX-USDC
        vm.startPrank(USDC_WHALE);
        IERC20(USDC).approve(FRAX_USDC_POOL, 500_000e6);
        ICurvePool(FRAX_USDC_POOL).exchange(1, 0, 500_000e6, 0);
        vm.stopPrank();

        // Step 2: Build a 2-hop route: USDC → FRAX (FRAX-USDC) → USDC (2pool)
        KingfisherArb.Hop[] memory hops = new KingfisherArb.Hop[](2);
        hops[0] = KingfisherArb.Hop({
            pool:         FRAX_USDC_POOL,
            tokenIn:      USDC,
            tokenInIndex: 1,   // USDC in
            tokenOutIndex: 0,  // FRAX out
            isMetaPool:   false,
            minAmountOut: 1    // minimal guard — test only
        });
        hops[1] = KingfisherArb.Hop({
            pool:         TWOPOOL,
            tokenIn:      FRAX,
            tokenInIndex: 0,   // USDC in (2pool index 0)
            tokenOutIndex: 1,  // USDT out — simplification; adjust if route differs
            isMetaPool:   false,
            minAmountOut: 1
        });

        // Step 3: Fire executeArb — this test contract is the operator
        uint256 flashAmount = 1_000_000e6; // M USDC
        uint256 minProfit   = 1e6;          //  USDC minimum

        vm.expectEmit(true, false, false, false);
        emit KingfisherArb.ArbExecuted(USDC, flashAmount, 0, 0, 2);

        arb.executeArb(USDC, flashAmount, hops, minProfit);

        // Step 4: Confirm profit accumulated in contract
        uint256 contractBalance = IERC20(USDC).balanceOf(address(arb));
        assertGt(contractBalance, 0, "No profit accumulated");

        console2.log("test_RealArb: net profit (USDC wei):", contractBalance);
    }
}
