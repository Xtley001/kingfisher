// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import {SafeERC20} from "@openzeppelin/contracts/token/ERC20/utils/SafeERC20.sol";
import {Ownable2Step, Ownable} from "@openzeppelin/contracts/access/Ownable2Step.sol";
import {IAavePool, IFlashLoanSimpleReceiver} from "./interfaces/IAavePool.sol";
import {IBalancerVault, IFlashLoanRecipient} from "./interfaces/IBalancerVault.sol";
import {ICurvePool, ICurveMetaPool} from "./interfaces/ICurvePool.sol";

/**
 * @title KingfisherArb v2.1
 * @notice Atomic flash loan arbitrage across Curve stablecoin pools on Arbitrum One.
 *
 * Features:
 *   - Dual borrow sources: Balancer V2 (0% fee, zero-margin loss) and Aave V3 fallback (5 bps).
 *   - Typed custom errors for -50 gas per check and off-chain 4-byte selector decoding.
 *   - Transient storage reentrancy lock (EIP-1153 TSTORE/TLOAD) saving 19,900 gas.
 *   - Yul assembly hot path in callbacks and _approve.
 *   - Operator / owner separation: hot bot wallet executes arbs; cold wallet withdraws and manages allowlists.
 */
contract KingfisherArb is IFlashLoanSimpleReceiver, IFlashLoanRecipient, Ownable2Step {
    using SafeERC20 for IERC20;

    // ─── Custom Errors ────────────────────────────────────────────────────────

    error NotOperator();
    error NotAavePool();
    error NotBalancerVault();
    error BalancerDisabled();
    error BadInitiator();
    error ContractPaused();
    error ZeroAmount();
    error InvalidRouteLength(uint256 length);
    error PoolNotAllowed(address pool);
    error PoolUnhealthy(address pool);
    error ZeroInputAtHop(uint256 hopIndex);
    error ZeroMinAmountOut(uint256 hopIndex);
    error ProfitBelowMin(uint256 got, uint256 minRequired);
    error NothingToWithdraw(address token);
    error EthTransferFailed();
    error ZeroAddress();

    // ─── Transient Storage Reentrancy Lock (EIP-1153) ─────────────────────────

    uint256 private constant _REENTRANCY_SLOT = 0x52454e5452414e4359; // "REENTRANCY"

    modifier nonReentrant() {
        assembly {
            if tload(_REENTRANCY_SLOT) { revert(0, 0) }
            tstore(_REENTRANCY_SLOT, 1)
        }
        _;
        assembly { tstore(_REENTRANCY_SLOT, 0) }
    }

    // ─── Immutables ──────────────────────────────────────────────────────────

    IAavePool      public immutable AAVE_POOL;
    IBalancerVault public immutable BALANCER_VAULT;

    // ─── Constants ───────────────────────────────────────────────────────────

    uint256 public constant MAX_HOPS = 4;

    // ─── Mutable State ───────────────────────────────────────────────────────

    address public operator;
    uint256 public minProfitWei;
    bool    public paused;
    mapping(address => bool) public allowedPools;

    // ─── Route Encoding ──────────────────────────────────────────────────────

    struct Hop {
        address pool;
        address tokenIn;
        int128  tokenInIndex;
        int128  tokenOutIndex;
        bool    isMetaPool;
        uint256 minAmountOut;
    }

    struct RouteParams {
        Hop[]   hops;
        uint256 minProfit;
    }

    // ─── Events ──────────────────────────────────────────────────────────────

    event ArbExecuted(
        address indexed flashToken,
        uint256         flashAmount,
        uint256         flashFee,
        uint256         netProfit,
        uint256         hopsCount
    );
    event PoolAllowed(address indexed pool, bool allowed);
    event MinProfitUpdated(uint256 newMinProfitWei);
    event PausedUpdated(bool paused);
    event ProfitWithdrawn(address indexed token, uint256 amount, address indexed to);
    event OperatorUpdated(address indexed oldOperator, address indexed newOperator);

    // ─── Constructor ─────────────────────────────────────────────────────────

    constructor(
        address          _aavePool,
        address          _balancerVault,
        uint256          _minProfitWei,
        address[] memory _initialPools
    ) Ownable(msg.sender) {
        if (_aavePool == address(0)) revert ZeroAddress();

        AAVE_POOL      = IAavePool(_aavePool);
        BALANCER_VAULT = IBalancerVault(_balancerVault);
        operator       = msg.sender;
        minProfitWei   = _minProfitWei;

        for (uint256 i = 0; i < _initialPools.length; i++) {
            if (_initialPools[i] == address(0)) revert ZeroAddress();
            allowedPools[_initialPools[i]] = true;
            emit PoolAllowed(_initialPools[i], true);
        }
    }

    // ─── Modifiers ────────────────────────────────────────────────────────────

    modifier onlyOperator() {
        if (msg.sender != operator) revert NotOperator();
        _;
    }

    // ─── External: Initiate Arb via Aave V3 ───────────────────────────────────

    function executeArb(
        address        flashToken,
        uint256        flashAmount,
        Hop[] calldata hops,
        uint256        minProfit
    ) external onlyOperator nonReentrant returns (uint256 netProfit) {
        _validatePreExecution(flashAmount, hops, minProfit);

        uint256 balanceBefore = IERC20(flashToken).balanceOf(address(this));
        bytes memory params = abi.encode(RouteParams({hops: hops, minProfit: minProfit}));

        AAVE_POOL.flashLoanSimple(address(this), flashToken, flashAmount, params, 0);

        uint256 balanceAfter = IERC20(flashToken).balanceOf(address(this));
        netProfit = balanceAfter > balanceBefore ? balanceAfter - balanceBefore : 0;
    }

    // ─── External: Initiate Arb via Balancer V2 (0% Fee) ───────────────────────

    function executeArbBalancer(
        address        flashToken,
        uint256        flashAmount,
        Hop[] calldata hops,
        uint256        minProfit
    ) external onlyOperator nonReentrant returns (uint256 netProfit) {
        if (address(BALANCER_VAULT) == address(0)) revert BalancerDisabled();
        _validatePreExecution(flashAmount, hops, minProfit);

        uint256 balanceBefore = IERC20(flashToken).balanceOf(address(this));
        bytes memory params = abi.encode(RouteParams({hops: hops, minProfit: minProfit}), address(this));

        IERC20[] memory tokens = new IERC20[](1);
        tokens[0] = IERC20(flashToken);
        uint256[] memory amounts = new uint256[](1);
        amounts[0] = flashAmount;

        BALANCER_VAULT.flashLoan(address(this), tokens, amounts, params);

        uint256 balanceAfter = IERC20(flashToken).balanceOf(address(this));
        netProfit = balanceAfter > balanceBefore ? balanceAfter - balanceBefore : 0;
    }

    // ─── Shared Validation ────────────────────────────────────────────────────

    function _validatePreExecution(
        uint256        flashAmount,
        Hop[] calldata hops,
        uint256        minProfit
    ) internal view {
        if (paused) revert ContractPaused();
        if (flashAmount == 0) revert ZeroAmount();
        if (hops.length < 2 || hops.length > MAX_HOPS)
            revert InvalidRouteLength(hops.length);
        if (minProfit < minProfitWei)
            revert ProfitBelowMin(minProfit, minProfitWei);

        for (uint256 i = 0; i < hops.length; i++) {
            if (!allowedPools[hops[i].pool])
                revert PoolNotAllowed(hops[i].pool);
            if (ICurvePool(hops[i].pool).get_virtual_price() < 1e18)
                revert PoolUnhealthy(hops[i].pool);
            if (hops[i].tokenIn == address(0))
                revert ZeroAddress();
            if (hops[i].minAmountOut == 0)
                revert ZeroMinAmountOut(i);
        }
    }

    // ─── Aave Callback: executeOperation ──────────────────────────────────────

    function executeOperation(
        address        asset,
        uint256        amount,
        uint256        premium,
        address        initiator,
        bytes calldata params
    ) external override returns (bool) {
        address _aavePool = address(AAVE_POOL);
        assembly {
            if iszero(eq(caller(), _aavePool)) {
                // NotAavePool()
                mstore(0x00, 0x9f87fad700000000000000000000000000000000000000000000000000000000)
                revert(0x00, 0x04)
            }
            if iszero(eq(initiator, address())) {
                // BadInitiator()
                mstore(0x00, 0x5c5eb8de00000000000000000000000000000000000000000000000000000000)
                revert(0x00, 0x04)
            }
        }

        RouteParams memory route = abi.decode(params, (RouteParams));
        uint256 balanceBefore = IERC20(asset).balanceOf(address(this));

        _executeHops(route.hops);

        // Repay Aave (amount + premium)
        uint256 repayAmount = amount + premium;
        _approve(asset, address(AAVE_POOL), repayAmount);

        // Profit guard
        uint256 balanceAfter = IERC20(asset).balanceOf(address(this));
        uint256 netProfit = balanceAfter > (balanceBefore + premium)
            ? balanceAfter - balanceBefore - premium
            : 0;

        if (netProfit < route.minProfit)
            revert ProfitBelowMin(netProfit, route.minProfit);

        emit ArbExecuted(asset, amount, premium, netProfit, route.hops.length);
        return true;
    }

    // ─── Balancer Callback: receiveFlashLoan (0% Fee) ─────────────────────────

    function receiveFlashLoan(
        IERC20[] memory tokens,
        uint256[] memory amounts,
        uint256[] memory feeAmounts,
        bytes memory    userData
    ) external override {
        address _vault = address(BALANCER_VAULT);
        assembly {
            if iszero(eq(caller(), _vault)) {
                // NotBalancerVault()
                mstore(0x00, 0x8a92bb1c00000000000000000000000000000000000000000000000000000000)
                revert(0x00, 0x04)
            }
        }

        (RouteParams memory route, address initiator) = abi.decode(userData, (RouteParams, address));
        if (initiator != address(this)) revert BadInitiator();

        address asset = address(tokens[0]);
        uint256 balanceBefore = IERC20(asset).balanceOf(address(this));

        _executeHops(route.hops);

        // Repay Balancer Vault directly
        uint256 repayAmount = amounts[0] + feeAmounts[0];
        tokens[0].safeTransfer(address(BALANCER_VAULT), repayAmount);

        // Profit guard (0 bps fee)
        uint256 balanceAfter = IERC20(asset).balanceOf(address(this));
        uint256 netProfit = balanceAfter > (balanceBefore + feeAmounts[0])
            ? balanceAfter - balanceBefore - feeAmounts[0]
            : 0;

        if (netProfit < route.minProfit)
            revert ProfitBelowMin(netProfit, route.minProfit);

        emit ArbExecuted(asset, amounts[0], feeAmounts[0], netProfit, route.hops.length);
    }

    // ─── Shared Swap Hop Execution Loop ──────────────────────────────────────

    function _executeHops(Hop[] memory hops) internal {
        for (uint256 i = 0; i < hops.length; i++) {
            Hop memory hop = hops[i];
            address tokenIn  = hop.tokenIn;
            uint256 amountIn = IERC20(tokenIn).balanceOf(address(this));
            if (amountIn == 0) revert ZeroInputAtHop(i);

            _approve(tokenIn, hop.pool, amountIn);

            if (hop.isMetaPool) {
                ICurveMetaPool(hop.pool).exchange_underlying(
                    hop.tokenInIndex, hop.tokenOutIndex, amountIn, hop.minAmountOut
                );
            } else {
                ICurvePool(hop.pool).exchange(
                    hop.tokenInIndex, hop.tokenOutIndex, amountIn, hop.minAmountOut
                );
            }
        }
    }

    // ─── Inline Assembly Token Approve ────────────────────────────────────────

    function _approve(address token, address spender, uint256 amount) internal {
        assembly {
            let ptr := mload(0x40)
            mstore(ptr, 0x095ea7b300000000000000000000000000000000000000000000000000000000)
            mstore(add(ptr, 4), spender)
            mstore(add(ptr, 36), 0)
            pop(call(gas(), token, 0, ptr, 68, 0, 0))
            mstore(add(ptr, 36), amount)
            if iszero(call(gas(), token, 0, ptr, 68, 0, 0)) {
                revert(0, 0)
            }
        }
    }

    // ─── Owner Controls & Withdrawals ─────────────────────────────────────────

    function setOperator(address _operator) external onlyOwner {
        if (_operator == address(0)) revert ZeroAddress();
        emit OperatorUpdated(operator, _operator);
        operator = _operator;
    }

    function setPoolAllowed(address pool, bool allowed) external onlyOwner {
        allowedPools[pool] = allowed;
        emit PoolAllowed(pool, allowed);
    }

    function setPoolsAllowed(address[] calldata pools, bool allowed) external onlyOwner {
        for (uint256 i = 0; i < pools.length; i++) {
            allowedPools[pools[i]] = allowed;
            emit PoolAllowed(pools[i], allowed);
        }
    }

    function setMinProfitWei(uint256 _minProfitWei) external onlyOwner {
        minProfitWei = _minProfitWei;
        emit MinProfitUpdated(_minProfitWei);
    }

    function setPaused(bool _paused) external onlyOwner {
        paused = _paused;
        emit PausedUpdated(_paused);
    }

    function withdrawProfit(address token) external onlyOwner {
        uint256 balance = IERC20(token).balanceOf(address(this));
        if (balance == 0) revert NothingToWithdraw(token);
        IERC20(token).safeTransfer(owner(), balance);
        emit ProfitWithdrawn(token, balance, owner());
    }

    function withdrawProfitBatch(address[] calldata tokens) external onlyOwner {
        for (uint256 i = 0; i < tokens.length; i++) {
            uint256 balance = IERC20(tokens[i]).balanceOf(address(this));
            if (balance > 0) {
                IERC20(tokens[i]).safeTransfer(owner(), balance);
                emit ProfitWithdrawn(tokens[i], balance, owner());
            }
        }
    }

    function rescueETH() external onlyOwner {
        uint256 balance = address(this).balance;
        if (balance == 0) revert NothingToWithdraw(address(0));
        (bool ok, ) = owner().call{value: balance}("");
        if (!ok) revert EthTransferFailed();
    }

    receive() external payable {}
}
