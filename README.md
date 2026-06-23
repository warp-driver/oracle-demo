# oracle-demo

Multi-round price oracle demonstrating Warp Drive's cron, Stellar-event,
and composition-event triggers across BTC/USD and ETH/USD CoinGecko spot
data. A single on-chain `OracleContract` on Stellar testnet receives
quorum-signed TWAPs and a final median per request, signed by the
operator quorum using ed25519 / SEP-53.

This repo is a tech-demo: every piece (the three WASI 0.2 circuits, the
shared aggregator, the Soroban handler, and the React frontend) is
small, standalone, and reads top-to-bottom in one sitting.

## The flow

```
              ┌──────────────────────────────────────────────────────┐
              │                                                      │
   (cron)  ──▶ │ 1. fetch_prices  ─ cron-circuit                      │
              │    every 10 min, GET api.coingecko.com,              │
              │    write {btc,eth} samples to wasi:keyvalue          │
              │                                                      │
   user ───▶  ┌──────────────────────────────────────────────────────┐
   request   │ 2. OracleContract.request_twap(asset, range_secs)     │
              │    emits `TwapRequest` (topic[0]=symbol "twapreq",   │
              │    topic[1]=u64 request_id)                          │
              └──────────────────────────────────────────────────────┘
                                  │
   (stellar event) ──▶ ┌──────────────────────────────────────────────┐
                       │ 3. compute_twap ─ twap-circuit                │
                       │    geo-mean of samples in [now − range, now], │
                       │    emits XDR Round2Payload, submitted by      │
                       │    the shared aggregator as a single-signer   │
                       │    `submit_round2` tx to OracleContract       │
                       └──────────────────────────────────────────────┘
                                  │
                       once ≥ quorum operators have submitted:
                       OracleContract emits `Round2Ready`
                       (topic[0]=symbol "r2ready", topic[1]=request_id)
                                  │
   (stellar event) ──▶ ┌──────────────────────────────────────────────┐
                       │ 4. compute_median ─ median-circuit            │
                       │    deterministic median across the bundle's   │
                       │    attestations; emits XDR FinalPayload,      │
                       │    submitted via the shared aggregator as a   │
                       │    quorum-signed `submit_final` tx            │
                       └──────────────────────────────────────────────┘
                                  │
                       OracleContract.final_twap(id) → Option<i128>
                       OracleContract.latest(asset)  → Option<LatestTwap>
```

Round 1 is cron-driven and pure: no submission, just kv writes. Round 2
is event-driven and per-operator single-signer. Round 3 is event-driven
(the composition event from round 2) and quorum-signed. The same
aggregator wasm is reused for rounds 2 and 3; only the workflow
config differs.

## What you need installed

| Tool | Why | Get it |
|---|---|---|
| Rust (toolchain pinned in `rust-toolchain.toml`) | builds contracts + components | https://rustup.rs |
| `cargo-component` | compiles components to `wasm32-wasip1` | `cargo install cargo-component` |
| `stellar` CLI | testnet keys, deploy, invoke | https://developers.stellar.org/docs/tools/developer-tools/cli/install-cli |
| Docker | runs `warpdrive-stellar-middleware` to deploy the ed25519 stack | https://docs.docker.com/engine/install/ |
| Node 20+ + `yarn` | builds + runs the frontend | https://nodejs.org/ , `npm i -g yarn` |
| `wkg` | fetches WIT dependencies from wa.dev | `cargo install wkg` |
| `task` | runs `Taskfile.yml` | https://taskfile.dev/installation/ |
| `warpdrive-cli` + `warpdrive` daemon | upload components, run the node | from `../warpdrive`: `cargo install --path packages/cli --locked && cargo install --path packages/warpdrive --locked` |
| `jq` | the Taskfile mangles JSON in a few places | `apt install jq` / `brew install jq` |

## Quickstart (single-operator, testnet)

A funded testnet key, one warpdrive node, one signer. About 5 minutes
from clone to UI:

```bash
# 1. WIT deps (one-time)
task fetch-wit

# 2. Generate keys + write .env
./scripts/bootstrap-keys.sh > .env
set -a; source .env; set +a

# 3. Phase 1: build everything + deploy on-chain contracts
task deploy
# -> out/deploy.json, out/oracle.json

# 4. Start the warpdrive node (leave running in another terminal)
#    cd <repo>; set -a; source .env; set +a
#    task run-node
# Wait for "Stellar chain [stellar:testnet] is healthy".

# 5. Phase 2: upload components, build + register service.json
task wire-service

# 6. Register the operator's pubkey + set 1/1 threshold
task register-signer

# 7. Hand the oracle contract id to the frontend
task frontend-config

# 8. Run the UI (opens http://localhost:5173)
task frontend-dev
```

Every step uses the same `DEPLOYER_SECRET` + `WARPDRIVE_SIGNING_MNEMONIC`
that `bootstrap-keys.sh` wrote into `.env`. If a task complains about a
missing env var, you forgot the `set -a; source .env; set +a` in that
shell.

## Connecting wallets

The frontend talks to two wallets:

- **Freighter** (Stellar testnet). Used to call
  `OracleContract.request_twap(asset, range_secs)`. Install from
  https://freighter.app, switch the dropdown to **Test Network**, and
  fund the account at https://friendbot.stellar.org.
- **MetaMask** (any network — the demo does NOT submit on Ethereum).
  Used only to sign a static message proving Ethereum wallet ownership,
  to demonstrate Warp Drive's cross-chain auth shape. Mainnet or any
  testnet network works; no balance is required.

The UI displays the latest TWAP per asset, the running median once the
quorum settles, and the raw round-2 attestation bundle so you can verify
each signer's signature manually if you want.

## What lives where

```
oracle-demo/
├── Cargo.toml                        # workspace for the 3 Soroban contracts only
├── Taskfile.yml                      # every build/deploy/run task
├── INSTRUCTION                       # ≤60-line cheat-sheet for returning devs
├── warpdrive.toml                    # node config (chains, p2p, gateway)
├── rust-toolchain.toml               # pinned Rust + wasm targets
├── scripts/
│   ├── bootstrap-keys.sh             # mint demo identities + emit .env block
│   └── build-service.sh              # declarative warpdrive-cli → service.json
├── contracts/
│   ├── oracle/                       # the handler (request_twap, submit_round2, submit_final)
│   ├── ed25519-verification/         # vendored quorum signature checker
│   └── ed25519-security/             # vendored signer registry + threshold
├── components/
│   ├── cron-circuit/                 # round 1: poll CoinGecko, write samples
│   ├── twap-circuit/                 # round 2: geo-mean → Round2Payload
│   ├── median-circuit/               # round 3: median → FinalPayload
│   └── aggregator/                   # shared Stellar submitter (oracle-aggregator)
├── wit-definitions/
│   └── wit/
│       ├── world.wit                 # circuit-world + aggregator-world
│       └── deps/                     # populated by `task fetch-wit`
├── frontend/                         # React + Vite UI
├── service/                          # service.json built per deploy (gitignored)
└── out/                              # per-deploy artefacts (gitignored)
```

## Multi-operator deployment

Out of scope for the quickstart but supported end-to-end. The pattern
is:

1. Edit `warpdrive.toml`: replace `[warpdrive.p2p.local]` with the
   `[warpdrive.p2p.remote]` template at the bottom. Operator 1 leaves
   `bootstrap_nodes = []`; operators 2 and 3 list operator 1's multiaddr.
2. Every operator runs `task deploy` against the **same** project_root
   (only operator 1 actually deploys; the others just need the same
   `out/deploy.json` + `out/oracle.json` copied in).
3. Every operator runs `task run-node` and `task register-signer` once.
   The Taskfile's `register-signer` posts that operator's pubkey to
   the on-chain security contract.
4. Operator 1 pins service.json to IPFS: `PINATA_JWT=... task
   publish-service`. The other nodes pick it up via
   `project_root.service_uri()` on their next chain poll. (Get a Pinata
   JWT at https://app.pinata.cloud > API Keys, permission
   `pinFileToIPFS`.)
5. Adjust threshold once you know how many signers are registered:
   `THRESHOLD_NUM=2 THRESHOLD_DEN=3 task set-threshold`.

The on-chain quorum (default 4/5 of registered signers, set at oracle
deploy time via `QUORUM_NUM` / `QUORUM_DEN`) is independent of the
verification contract threshold. Set them together — for 3-operator the
typical setup is `QUORUM_NUM=2 QUORUM_DEN=3 task deploy-oracle` and
`THRESHOLD_NUM=2 THRESHOLD_DEN=3 task register-signer`.

## Troubleshooting

- **`task fetch-wit` fails with 401/404 from wa.dev.** The WIT deps live
  on https://wa.dev/warpdrive — they are public, but `wkg` follows the
  registry from `wit-definitions/wkg.toml`. Check that
  `wit-definitions/wkg.toml` exists and that you can `curl
  https://wa.dev/warpdrive/vectr` with no auth. If you're behind a
  corp proxy, point `wkg` at it via `HTTPS_PROXY`.
- **`stellar contract build` fails with `target wasm32v1-none not
  installed`.** The pinned toolchain in `rust-toolchain.toml` lists both
  `wasm32-wasip1` (for components) and `wasm32v1-none` (for contracts).
  Force it: `rustup target add wasm32v1-none --toolchain $(cat
  rust-toolchain.toml | grep channel | cut -d'"' -f2)`.
- **`task register-service` 404s.** The node either isn't running
  (start it with `task run-node`) or has `dev_endpoints_enabled =
  false` in `warpdrive.toml`. The shipped config has it on.
- **`task fetch-signer` returns an empty pubkey.** The service hasn't
  been registered yet — run `task register-service` first.
- **`stellar contract deploy ... -- --verification_contract` fails with
  "missing required argument".** `out/deploy.json` is empty or
  malformed. Re-run `task deploy-middleware` and confirm
  `jq .contracts out/deploy.json` shows three addresses.
- **`task register-handler` fails with "NotAHandler" or "NotOurContract".**
  The oracle's `verification_contract()` doesn't match project_root's.
  This happens if you re-ran `deploy-middleware` after
  `deploy-oracle`. Re-run `deploy-oracle` to redeploy against the new
  verification contract, then `register-handler` again.

## See also

- [`INSTRUCTION`](./INSTRUCTION) — bare cheat-sheet for returning devs.
- [`warpdrive`](https://github.com/warp-driver/warpdrive) — the engine.
- [`warpdrive-contracts`](https://github.com/warp-driver/warpdrive-contracts) —
  upstream source of the ed25519 contracts + project_root.

## License

GPL-3.0.
