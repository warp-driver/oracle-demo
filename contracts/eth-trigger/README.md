# eth-trigger

`TwapTrigger.sol` is the Sepolia-side entry point of the oracle-demo's
MetaMask bridge path. It is intentionally trivial: a single
state-less `request(string asset, uint32 rangeSecs)` external that
emits `TwapRequested(asset, rangeSecs, msg.sender)`.

A MetaMask user calls `request(...)` on Sepolia; both Warp Drive
operator nodes independently observe the resulting log via their
`eth-bridge-circuit`, agree on an identical `BridgeTriggerPayload`
under full quorum, and submit it to the Stellar `OracleContract`,
which then runs the standard Round 2 / Round 3 / Final pipeline.

The Freighter (Stellar-native) path is unaffected; the bridge is
additive.

## Event

- Signature: `TwapRequested(string,uint32,address)`
- topic0 (keccak256): `0xa02a28cf01f2361e6c38ce664ac5287b7b3dbb7d6368d3646d5e865d82cfa88a`
- `topics[1]` is the indexed `requester` address (left-padded to 32 bytes).
- `data` is ABI-encoded `(string asset, uint32 rangeSecs)`.

Recompute with: `cast keccak "TwapRequested(string,uint32,address)"`.

## Build & deploy

```sh
forge build                       # produces out/TwapTrigger.sol/TwapTrigger.json
task deploy-eth-trigger           # primary deploy path (forge create + jq)
```

`task deploy-eth-trigger` (defined at the repo root by another agent)
writes `out/eth-trigger.json = { address, event_hash, chain_id }`,
which the service-config script and frontend both consume.

`script/Deploy.s.sol` is a backup forge-script entry; the Taskfile
target uses `forge create` directly.
