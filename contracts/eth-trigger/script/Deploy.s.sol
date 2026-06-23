// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {TwapTrigger} from "../src/TwapTrigger.sol";

/// @dev Minimal subset of forge-std's `Vm` cheatcode interface, inlined
///      so this script is zero-dependency (foundry.toml keeps
///      `libs = []`). All cheatcodes live at the canonical address
///      derived from `keccak256("hevm cheat code")`.
interface Vm {
    function envUint(string calldata name) external view returns (uint256);
    function startBroadcast(uint256 privateKey) external;
    function stopBroadcast() external;
}

/// @dev Minimal `console.log` replacement: emits a structured event so
///      `forge script` traces still surface the deployed address
///      without depending on forge-std's HEVM console hack.
contract DeployLogger {
    event Deployed(string what, address at);
}

/// @title  Deploy
/// @notice Backup / documentation example of how to deploy TwapTrigger
///         to Sepolia. The demo's primary deploy path is `forge create`
///         driven from `task deploy-eth-trigger`; this script exists so
///         operators who prefer scripted deploys have a one-shot entry.
///
///         Required env:
///           SEPOLIA_DEPLOYER_KEY  — 0x-prefixed funded Sepolia private key.
///
///         Usage:
///           forge script script/Deploy.s.sol:Deploy \
///               --rpc-url "$SEPOLIA_RPC_URL" --broadcast
contract Deploy is DeployLogger {
    Vm internal constant vm =
        Vm(address(uint160(uint256(keccak256("hevm cheat code")))));

    function run() external returns (TwapTrigger trigger) {
        uint256 deployerKey = vm.envUint("SEPOLIA_DEPLOYER_KEY");

        vm.startBroadcast(deployerKey);
        trigger = new TwapTrigger();
        vm.stopBroadcast();

        emit Deployed("TwapTrigger", address(trigger));
    }
}
