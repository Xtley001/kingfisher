// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import {SafeERC20} from "@openzeppelin/contracts/token/ERC20/utils/SafeERC20.sol";

interface IMorpho {
    function flashLoan(address token, uint256 assets, bytes calldata data) external;
}

/**
 * @title KingfisherMonad
 * @notice Monad executor for Strategy B (PULSE Oracle Repricing Lag).
 *         Borrows flash liquidity from Morpho Blue (0 fee), executes venue swaps,
 *         validates profit, and repays Morpho in a single atomic transaction.
 */
contract KingfisherMonad {
    using SafeERC20 for IERC20;

    // ─── Custom Errors ────────────────────────────────────────────────────────
    error NotOwner();
    error NotOperator();
    error ContractPaused();
    error NotMorphoVault();
    error InsufficientProfit(uint256 received, uint256 required);
    error ExecutionFailed(string reason);
    error ZeroAddress();
    error VenueNotAllowed(address venue);

    // ─── Immutables & State ───────────────────────────────────────────────────
    address public immutable MORPHO_VAULT;
    address public owner;
    address public operator;
    address public profitWallet;
    bool public paused;
    uint256 public minProfitWei;
    mapping(address => bool) public allowedVenues;

    // Transient execution state
    address private _initiator;
    address private _flashToken;
    uint256 private _minExpectedOut;

    // ─── Events ───────────────────────────────────────────────────────────────
    event Executed(address indexed token, uint256 amount, uint256 netProfit);
    event OperatorUpdated(address indexed oldOperator, address indexed newOperator);
    event ProfitWalletUpdated(address indexed oldWallet, address indexed newWallet);
    event PausedSet(bool indexed paused);
    event MinProfitUpdated(uint256 newMinProfit);
    event VenueAllowed(address indexed venue, bool allowed);

    modifier onlyOwner() {
        if (msg.sender != owner) revert NotOwner();
        _;
    }

    modifier onlyOperator() {
        if (msg.sender != operator && msg.sender != owner) revert NotOperator();
        _;
    }

    modifier whenNotPaused() {
        if (paused) revert ContractPaused();
        _;
    }

    constructor(
        address _morpho,
        address _profitWallet,
        uint256 _minProfitWei
    ) {
        if (_morpho == address(0) || _profitWallet == address(0)) revert ZeroAddress();
        MORPHO_VAULT = _morpho;
        profitWallet = _profitWallet;
        minProfitWei = _minProfitWei;
        owner = msg.sender;
        operator = msg.sender;
    }

    // ─── Execution Entrypoint ─────────────────────────────────────────────────

    struct SwapHop {
        address target;      // Router or Venue contract
        bytes payload;       // Encoded call to venue router
        address tokenIn;
        address tokenOut;
    }

    /**
     * @notice Execute an atomic flash-arb opportunity.
     * @param token Flash borrow asset (e.g. USDC)
     * @param amount Amount to flash loan
     * @param hops Sequence of swap hops
     * @param minProfit Minimum profit required after loan repayment
     */
    function executePulse(
        address token,
        uint256 amount,
        SwapHop[] calldata hops,
        uint256 minProfit
    ) external onlyOperator whenNotPaused {
        _initiator = msg.sender;
        _flashToken = token;
        _minExpectedOut = amount + (minProfit >= minProfitWei ? minProfit : minProfitWei);

        bytes memory params = abi.encode(hops);
        IMorpho(MORPHO_VAULT).flashLoan(token, amount, params);

        // Clear transient state
        _initiator = address(0);
        _flashToken = address(0);
        _minExpectedOut = 0;
    }

    /**
     * @notice Morpho Blue native flash loan callback.
     * @param assets Amount borrowed
     * @param data Encoded SwapHop[]
     */
    function onMorphoFlashLoan(uint256 assets, bytes calldata data) external {
        if (msg.sender != MORPHO_VAULT) revert NotMorphoVault();

        SwapHop[] memory hops = abi.decode(data, (SwapHop[]));

        // Execute venue hops
        for (uint256 i = 0; i < hops.length; i++) {
            SwapHop memory hop = hops[i];
            if (!allowedVenues[hop.target]) {
                revert VenueNotAllowed(hop.target);
            }
            if (hop.tokenIn != address(0) && hop.target != address(0)) {
                uint256 amountIn = IERC20(hop.tokenIn).balanceOf(address(this));
                IERC20(hop.tokenIn).forceApprove(hop.target, amountIn);
            }
            (bool success, bytes memory ret) = hop.target.call(hop.payload);
            if (!success) {
                revert ExecutionFailed(string(ret));
            }
        }

        // Verify balance satisfies repayment + profit
        uint256 balanceAfter = IERC20(_flashToken).balanceOf(address(this));
        if (balanceAfter < _minExpectedOut) {
            revert InsufficientProfit(balanceAfter, _minExpectedOut);
        }

        // Repay Morpho Blue (approve Morpho to pull assets, 0 fee)
        IERC20(_flashToken).forceApprove(MORPHO_VAULT, assets);

        uint256 netProfit = balanceAfter - assets;
        if (netProfit > 0 && profitWallet != address(this)) {
            IERC20(_flashToken).safeTransfer(profitWallet, netProfit);
        }

        emit Executed(_flashToken, assets, netProfit);
    }

    // ─── Admin Functions ──────────────────────────────────────────────────────

    function setVenueAllowed(address venue, bool allowed) external onlyOwner {
        if (venue == address(0)) revert ZeroAddress();
        allowedVenues[venue] = allowed;
        emit VenueAllowed(venue, allowed);
    }

    function setVenuesAllowed(address[] calldata venues, bool allowed) external onlyOwner {
        for (uint256 i = 0; i < venues.length; i++) {
            if (venues[i] == address(0)) revert ZeroAddress();
            allowedVenues[venues[i]] = allowed;
            emit VenueAllowed(venues[i], allowed);
        }
    }

    function setOperator(address newOperator) external onlyOwner {
        if (newOperator == address(0)) revert ZeroAddress();
        emit OperatorUpdated(operator, newOperator);
        operator = newOperator;
    }

    function setProfitWallet(address newProfitWallet) external onlyOwner {
        if (newProfitWallet == address(0)) revert ZeroAddress();
        emit ProfitWalletUpdated(profitWallet, newProfitWallet);
        profitWallet = newProfitWallet;
    }

    function setPaused(bool _paused) external onlyOperator {
        paused = _paused;
        emit PausedSet(_paused);
    }

    function setMinProfitWei(uint256 _minProfit) external onlyOwner {
        minProfitWei = _minProfit;
        emit MinProfitUpdated(_minProfit);
    }

    function sweep(address token) external onlyOwner {
        uint256 balance = IERC20(token).balanceOf(address(this));
        if (balance > 0) {
            IERC20(token).safeTransfer(owner, balance);
        }
    }
}
