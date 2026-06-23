// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

/// @title  TwapTrigger
/// @notice Sepolia-side entry point for the MetaMask bridge into the
///         oracle-demo. A user (any wallet, no allow-list) calls
///         `request(asset, rangeSecs)`; the contract simply emits a
///         `TwapRequested` event tagging the caller. Warp Drive operator
///         nodes watch this event with full-quorum agreement and bridge
///         it into a `request_twap` invocation on the Stellar
///         OracleContract.
///
///         Intentionally state-less: no constructor, no owner, no
///         storage. Spam protection is delegated to Sepolia gas costs
///         and to the aggregator's Stellar fee budget downstream.
contract TwapTrigger {
    /// Emitted whenever a MetaMask user requests a TWAP. The off-chain
    /// bridge circuit decodes `(asset, rangeSecs)` from the data area
    /// and the requester from `topics[1]`.
    ///
    /// keccak256("TwapRequested(string,uint32,address)") =
    /// 0xa02a28cf01f2361e6c38ce664ac5287b7b3dbb7d6368d3646d5e865d82cfa88a
    event TwapRequested(
        string asset,
        uint32 rangeSecs,
        address indexed requester
    );

    /// User-facing entry: any MetaMask wallet calls this on Sepolia,
    /// the warpdrive node bridges the event into a `request_twap` on
    /// the Stellar OracleContract.
    function request(string calldata asset, uint32 rangeSecs) external {
        emit TwapRequested(asset, rangeSecs, msg.sender);
    }
}
