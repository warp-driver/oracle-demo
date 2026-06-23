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
              │    default 30 s (demo), Submit::None.                │
              │    GET api.coingecko.com → wasi:keyvalue samples     │
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
                       │    emits XDR SubmissionPayload::Round2,       │
                       │    submitted by the shared aggregator as a    │
                       │    `verify_xlm` tx to OracleContract          │
                       └──────────────────────────────────────────────┘
                                  │
                       once ≥ threshold operators have submitted:
                       OracleContract emits `Round2Ready`
                       (topic[0]=symbol "r2ready", topic[1]=request_id)
                                  │
   (stellar event) ──▶ ┌──────────────────────────────────────────────┐
                       │ 4. compute_median ─ median-circuit            │
                       │    deterministic median across the bundle's   │
                       │    attestations; emits XDR                    │
                       │    SubmissionPayload::Final, submitted via    │
                       │    the shared aggregator as a quorum-signed   │
                       │    `verify_xlm` tx                            │
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

The quickstart runs one operator at 1/1 threshold. The whole point of
the architecture is N operators independently fetching CoinGecko,
computing TWAPs, and converging on a quorum-signed median. Two ways
to demo that.

### Two key numbers

The contract holds **two distinct fractions**. Set them together or
round-2 silently never releases:

| Knob | Where it lives | What it means |
|---|---|---|
| `QUORUM_NUM / QUORUM_DEN` | OracleContract constructor (default 4/5) | Round-2 release threshold = `ceil(signer_count × num / denom)`. Once that many per-Vectr attestations land, the contract emits `Round2Ready`. |
| `THRESHOLD_NUM / THRESHOLD_DEN` | ed25519-security contract (default 1/1) | Verification threshold. The aggregator must collect signatures whose summed weight ≥ `total_weight × num / denom` for `verify_xlm` to accept a submission. |

For a 5-operator 4-of-5 setup: `QUORUM_NUM=4 QUORUM_DEN=5 task
deploy-oracle` (Round 2 needs 4 of 5 single-sig attestations on chain)
AND `THRESHOLD_NUM=4 THRESHOLD_DEN=5 task set-threshold` (Round 3
`verify_xlm` accepts only a 4-of-5 quorum-signed envelope).

### Same-host: N operators on one machine

Smallest possible multi-op setup — useful for screenshotting `4/5
signed` without renting hardware. Each operator gets its own
`WARPDRIVE_HOME`, its own port, its own signing mnemonic; they
discover each other over mDNS on the same loopback.

```bash
cd ~/work/warpdrive/oracle-demo
set -a; source .env; set +a   # uses the existing DEPLOYER_SECRET

# 1. Re-deploy the oracle with a 4/5 release threshold.
QUORUM_NUM=4 QUORUM_DEN=5 task deploy-oracle
task register-handler
task frontend-config

# 2. Spin up 4 more nodes. Each gets its own home dir, port, mnemonic.
for i in 2 3 4 5; do
    mkdir -p out/op-$i
    cp warpdrive.toml out/op-$i/
    # Each node needs a unique [warpdrive] port + p2p listen_port; the
    # shipped warpdrive.toml lives at port 8000 / 9000. Bump by 10 per
    # operator so 1=8000+9000, 2=8010+9010, ...
    sed -i "s/^port = 8000/port = $((8000 + (i-1)*10))/" out/op-$i/warpdrive.toml
    sed -i "s/^listen_port = 9000/listen_port = $((9000 + (i-1)*10))/" out/op-$i/warpdrive.toml
    MNEMONIC=$(stellar keys generate "oracle-op-$i" --no-fund && \
               stellar keys show "oracle-op-$i" --phrase)
    cat > out/op-$i/.env <<EOF
DEPLOYER_SECRET=$DEPLOYER_SECRET
DEPLOYER_ADDRESS=$DEPLOYER_ADDRESS
WARPDRIVE_SIGNING_MNEMONIC="$MNEMONIC"
EOF
done

# 3. In FIVE separate terminals — one per operator — start a node.
# Terminal 1 (the one you already had running):
#     set -a; source .env; set +a; task run-node
# Terminals 2..5:
#     cd ~/work/warpdrive/oracle-demo
#     set -a; source out/op-N/.env; set +a
#     WARPDRIVE_HOME=out/op-N WARPDRIVE_DATA=out/op-N/data \
#         warpdrive --home out/op-N --port $((8000 + (N-1)*10))
# (replace N with 2, 3, 4, 5)

# 4. Back in terminal 1 — register each operator's pubkey on the
#    security contract. The register-signer task always reads the
#    LOCAL node's pubkey (port 8000), so we override the endpoint per
#    operator. Wait ~3 s between calls so testnet sees each tx land.
for i in 1 2 3 4 5; do
    PORT=$((8000 + (i-1)*10))
    WARPDRIVE_ENDPOINT="http://127.0.0.1:$PORT" task fetch-signer
    scripts/middleware.sh add-signer \
        --scheme ed25519 \
        --key "$(cat out/signer.pubkey)" \
        --weight 100 \
        --deploy-file /out/deploy.json \
        --via-project-root
    sleep 3
done

# 5. Set the verification threshold to 4/5.
THRESHOLD_NUM=4 THRESHOLD_DEN=5 task set-threshold

# 6. Each operator picks up service.json via the dispatcher. The
#    simplest path: pin once and let project_root point at it. The
#    other nodes poll project_root.service_uri() on their next chain
#    poll and self-register.
PINATA_JWT=... task publish-service
# (Get a free JWT at https://app.pinata.cloud → API Keys, scope
# pinFileToIPFS.)

# 7. Hit the UI. A single request_twap now fans out to all 5 nodes;
#    the Round 2 bundle on chain fills to 4 signers, OracleContract
#    emits Round2Ready, all 5 median circuits agree on the median, the
#    aggregator collects 4 sigs of weight 100 each (= 400 ≥ required
#    400), submits one verify_xlm with the quorum-signed envelope, and
#    Finalized lands. End to end < 60 s on testnet.
```

Cleanup: `for i in 2 3 4 5; do stellar keys rm oracle-op-$i; done && rm -rf out/op-{2,3,4,5}`.

### Multi-host: one box per operator

The pattern that ships to production. Each operator runs the repo on
their own server.

1. Operator 1 (the bootstrap node) runs the regular **single-operator
   quickstart** all the way through `task wire-service` so on-chain
   contracts exist and a service spec is registered with operator 1's
   dispatcher.
2. Operator 1 pins service.json to IPFS and points project_root at it:
   ```bash
   PINATA_JWT=... task publish-service
   ```
   (Get a free Pinata JWT at https://app.pinata.cloud → API Keys,
   permission `pinFileToIPFS`.)
3. Operators 2…N each clone the repo, copy operator 1's
   `out/deploy.json` + `out/oracle.json` into their `out/` (they need
   the contract addresses, not the keys), and edit
   `warpdrive.toml`:
   ```toml
   [warpdrive.p2p.remote]
   listen_port = 9000
   bootstrap_nodes = [
       "/ip4/<operator-1-public-ip>/tcp/9000/p2p/<operator-1-peer-id>",
   ]
   ```
   The peer_id is in operator 1's log: search for `peer_id: 12D3KooW…`.
4. Each operator 2…N runs:
   ```bash
   ./scripts/bootstrap-keys.sh > .env   # mints THIS operator's mnemonic
   set -a; source .env; set +a
   task fetch-wit
   task run-node                        # in another terminal
   task register-signer                 # adds this node's pubkey on chain
   ```
   They do NOT run `task deploy` — the contracts already exist.
   `register-service` is also skipped because every node fetches the
   spec from project_root.service_uri() automatically.
5. Once all N operators have registered their signers, operator 1
   raises the threshold to match:
   ```bash
   THRESHOLD_NUM=4 THRESHOLD_DEN=5 task set-threshold
   ```
   and re-deploys the oracle with the matching release ratio if
   needed:
   ```bash
   QUORUM_NUM=4 QUORUM_DEN=5 task deploy-oracle
   task register-handler
   task publish-service                 # the new oracle id changes service.json
   ```

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
