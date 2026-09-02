// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

/**
 * @notice Minimal Aave V3 IPool interface — only what Kingfisher needs.
 */
interface IAavePool {
    /**
     * @notice Initiate a flash loan of a single asset.
     * @param receiverAddress Contract implementing IFlashLoanSimpleReceiver
     * @param asset           Token to borrow
     * @param amount          Amount to borrow
     * @param params          Encoded params passed to executeOperation()
     * @param referralCode    Always 0
     */
    function flashLoanSimple(
        address receiverAddress,
        address asset,
        uint256 amount,
        bytes calldata params,
        uint16 referralCode
    ) external;

    /**
     * @notice Liquidate an unhealthy borrow position (Strategy A6).
     */
    function liquidationCall(
        address collateralAsset,
        address debtAsset,
        address user,
        uint256 debtToCover,
        bool receiveAToken
    ) external;

    /**
     * @notice Fetch user collateral, debt, and health factor.
     */
    function getUserAccountData(address user) external view returns (
        uint256 totalCollateralBase,
        uint256 totalDebtBase,
        uint256 availableBorrowsBase,
        uint256 currentLiquidationThreshold,
        uint256 ltv,
        uint256 healthFactor
    );
}

interface IFlashLoanSimpleReceiver {
    /**
     * @notice Called by Aave after flash loan funds are sent.
     * @param asset     The borrowed token address
     * @param amount    The borrowed amount
     * @param premium   The Aave fee (0.05% of amount)
     * @param initiator The address that called flashLoanSimple
     * @param params    The params passed to flashLoanSimple
     * @return True if operation succeeded — Aave will then pull amount + premium
     */
    function executeOperation(
        address asset,
        uint256 amount,
        uint256 premium,
        address initiator,
        bytes calldata params
    ) external returns (bool);
}
