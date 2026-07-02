// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

/**
 * @notice Standard Curve StableSwap pool (plain pools: 2pool, FRAX-USDC, crvUSD pools).
 * @dev These pools are written in Vyper. Use correct ABI — do not assume Solidity layout.
 */
interface ICurvePool {
    /// @notice Get amount of token j received for dx of token i (read-only, no state change)
    function get_dy(int128 i, int128 j, uint256 dx) external view returns (uint256);

    /// @notice Execute swap: sell dx of token i for token j
    /// @param min_dy Minimum acceptable output — set to 0 (profit guard in executeOperation)
    function exchange(int128 i, int128 j, uint256 dx, uint256 min_dy) external returns (uint256);

    /// @notice Raw balance of token at index i (in token's own decimals)
    function balances(uint256 i) external view returns (uint256);

    /// @notice Curve amplification coefficient — can ramp during governance changes
    function A() external view returns (uint256);

    /// @notice Virtual price — monotonically increasing, 1e18 base
    function get_virtual_price() external view returns (uint256);

    /// @notice Token address at index i
    function coins(uint256 i) external view returns (address);
}

/**
 * @notice Curve Metapool — wraps a base pool (e.g., 3CRV).
 * @dev exchange_underlying allows trading against base pool tokens directly.
 *      Used for MIM-3CRV, LUSD-3CRV type pools.
 */
interface ICurveMetaPool {
    function get_dy(int128 i, int128 j, uint256 dx) external view returns (uint256);
    function get_dy_underlying(int128 i, int128 j, uint256 dx) external view returns (uint256);
    function exchange(int128 i, int128 j, uint256 dx, uint256 min_dy) external returns (uint256);
    function exchange_underlying(int128 i, int128 j, uint256 dx, uint256 min_dy) external returns (uint256);
    function balances(uint256 i) external view returns (uint256);
    function A() external view returns (uint256);
    function get_virtual_price() external view returns (uint256);
    function coins(uint256 i) external view returns (address);
    function base_coins(uint256 i) external view returns (address);
}
