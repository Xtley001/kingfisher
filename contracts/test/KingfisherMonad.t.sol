// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {Test, console2} from "forge-std/Test.sol";
import {KingfisherMonad, IMorpho} from "../src/KingfisherMonad.sol";
import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import {ERC20} from "@openzeppelin/contracts/token/ERC20/ERC20.sol";

contract MockERC20 is ERC20 {
    constructor() ERC20("Mock USDC", "USDC") {
        _mint(msg.sender, 10_000_000e6);
    }

    function mint(address to, uint256 amount) external {
        _mint(to, amount);
    }
}

contract MockMorpho is IMorpho {
    function flashLoan(address token, uint256 assets, bytes calldata data) external override {
        // Transfer assets to caller
        IERC20(token).transfer(msg.sender, assets);

        // Invoke callback on borrower
        KingfisherMonad(msg.sender).onMorphoFlashLoan(assets, data);

        // Pull repayment from borrower
        IERC20(token).transferFrom(msg.sender, address(this), assets);
    }
}

contract MockRouter {
    address public token;
    uint256 public profitAmount;

    constructor(address _token, uint256 _profit) {
        token = _token;
        profitAmount = _profit;
    }

    function swap() external {
        // Simulates a profitable swap returning extra tokens
        MockERC20(token).mint(msg.sender, profitAmount);
    }
}

contract KingfisherMonadTest is Test {
    KingfisherMonad public arb;
    MockMorpho public morpho;
    MockERC20 public usdc;
    MockRouter public router;

    address public profitWallet = makeAddr("profitWallet");
    address public operator = makeAddr("operator");
    uint256 constant MIN_PROFIT = 5e6; // $5 USDC

    function setUp() public {
        morpho = new MockMorpho();
        usdc = new MockERC20();
        router = new MockRouter(address(usdc), 10e6); // gives $10 profit

        // Fund Morpho with liquidity
        usdc.mint(address(morpho), 1_000_000e6);

        arb = new KingfisherMonad(address(morpho), profitWallet, MIN_PROFIT);
        arb.setOperator(operator);
    }

    function test_Deployment() public view {
        assertEq(arb.MORPHO_VAULT(), address(morpho));
        assertEq(arb.profitWallet(), profitWallet);
        assertEq(arb.operator(), operator);
        assertEq(arb.owner(), address(this));
        assertFalse(arb.paused());
        assertEq(arb.minProfitWei(), MIN_PROFIT);
    }

    function test_UnauthorizedExecuteReverts() public {
        address attacker = makeAddr("attacker");
        KingfisherMonad.SwapHop[] memory hops = new KingfisherMonad.SwapHop[](0);

        vm.prank(attacker);
        vm.expectRevert(KingfisherMonad.NotOperator.selector);
        arb.executePulse(address(usdc), 1_000e6, hops, MIN_PROFIT);
    }

    function test_PausedExecuteReverts() public {
        vm.prank(operator);
        arb.setPaused(true);

        KingfisherMonad.SwapHop[] memory hops = new KingfisherMonad.SwapHop[](0);
        vm.prank(operator);
        vm.expectRevert(KingfisherMonad.ContractPaused.selector);
        arb.executePulse(address(usdc), 1_000e6, hops, MIN_PROFIT);
    }

    function test_CallbackOnlyFromMorpho() public {
        vm.prank(makeAddr("not-morpho"));
        vm.expectRevert(KingfisherMonad.NotMorphoVault.selector);
        arb.onMorphoFlashLoan(1_000e6, "");
    }

    function test_SuccessfulPulseExecution() public {
        KingfisherMonad.SwapHop[] memory hops = new KingfisherMonad.SwapHop[](1);
        hops[0] = KingfisherMonad.SwapHop({
            target: address(router),
            payload: abi.encodeWithSignature("swap()"),
            tokenIn: address(usdc),
            tokenOut: address(usdc)
        });

        uint256 profitBefore = usdc.balanceOf(profitWallet);

        vm.prank(operator);
        arb.executePulse(address(usdc), 100_000e6, hops, MIN_PROFIT);

        uint256 profitAfter = usdc.balanceOf(profitWallet);
        assertEq(profitAfter - profitBefore, 10e6);
    }
}
