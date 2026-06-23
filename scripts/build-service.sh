#!/usr/bin/env bash
#
# build-service.sh — assemble service/service.json declaratively via
# warpdrive-cli for the oracle-demo's three-workflow pipeline:
#
#   fetch_prices   cron @ "0 */10 * * * *"
#                  -> cron-circuit (HTTP: api.coingecko.com)
#                  -> submit none
#
#   compute_twap   stellar event (oracle, topic[0]=symbol "twapreq", topic[1]=*)
#                  -> twap-circuit (no http, no fs)
#                  -> submit aggregator (chain + service_handler config)
#
#   compute_median stellar event (oracle, topic[0]=symbol "r2ready", topic[1]=*)
#                  -> median-circuit (no http, no fs)
#                  -> submit aggregator (chain + service_handler config)
#
# Then sets the stellar service manager to project_root and appends the
# ed25519 / sep53 signature_kind via jq.
#
# Required inputs (relative to the oracle-demo repo root):
#
#   out/oracle.json       { "oracle": "C..." }              (from `task deploy-oracle`)
#   out/deploy.json       { "contracts": { "project_root": "C..." , ... } }
#                                                            (from `task deploy-middleware`)
#   out/cron.digest       64-hex content digest               (from `task upload-cron`)
#   out/twap.digest       64-hex content digest               (from `task upload-twap`)
#   out/median.digest     64-hex content digest               (from `task upload-median`)
#   out/aggregator.digest 64-hex content digest               (from `task upload-aggregator`)
#   SERVICE_FILE          target path (default: service/service.json)
#   TRIGGER_CHAIN         chain key for both Stellar-event triggers (default: stellar:testnet)
#   MANAGER_CHAIN         chain key for the service manager        (default: stellar:testnet)
#   CRON_SCHEDULE         cron expression for fetch_prices         (default: every 30 s)
#                         Set to "0 */10 * * * *" for the
#                         spec-aligned every-10-min cadence.

set -euo pipefail

SERVICE_FILE="${SERVICE_FILE:-service/service.json}"
TRIGGER_CHAIN="${TRIGGER_CHAIN:-stellar:testnet}"
MANAGER_CHAIN="${MANAGER_CHAIN:-stellar:testnet}"
CRON_SCHEDULE="${CRON_SCHEDULE:-*/30 * * * * *}"

# ── prerequisite files ────────────────────────────────────────────────

for f in out/oracle.json out/deploy.json \
         out/cron.digest out/twap.digest out/median.digest out/aggregator.digest; do
    test -s "$f" || { echo "missing $f — run the prerequisite task first" >&2; exit 1; }
done

ORACLE=$(jq -r .oracle out/oracle.json)
PROJECT_ROOT=$(jq -r .contracts.project_root out/deploy.json)
CRON_DIGEST=$(cat out/cron.digest)
TWAP_DIGEST=$(cat out/twap.digest)
MEDIAN_DIGEST=$(cat out/median.digest)
AGG_DIGEST=$(cat out/aggregator.digest)

for v in ORACLE PROJECT_ROOT CRON_DIGEST TWAP_DIGEST MEDIAN_DIGEST AGG_DIGEST; do
    test -n "${!v}" && test "${!v}" != "null" \
        || { echo "$v is empty — check the corresponding artefact under out/" >&2; exit 1; }
done

# ── initialise ─────────────────────────────────────────────────────────

mkdir -p "$(dirname "$SERVICE_FILE")"
rm -f "$SERVICE_FILE"

warpdrive-cli service -f "$SERVICE_FILE" init --name oracle-demo

# ── workflow 1: fetch_prices (cron → cron-circuit → submit none) ──────

warpdrive-cli service -f "$SERVICE_FILE" workflow add --id fetch_prices

warpdrive-cli service -f "$SERVICE_FILE" workflow trigger \
    --id fetch_prices set-cron \
    --schedule "$CRON_SCHEDULE"

warpdrive-cli service -f "$SERVICE_FILE" workflow component \
    --id fetch_prices set-source-digest --digest "$CRON_DIGEST"

warpdrive-cli service -f "$SERVICE_FILE" workflow component \
    --id fetch_prices permissions \
    --http-hosts api.coingecko.com

warpdrive-cli service -f "$SERVICE_FILE" workflow submit \
    --id fetch_prices set-none

# ── workflow 2: compute_twap (twapreq event → twap-circuit → aggregator) ─

warpdrive-cli service -f "$SERVICE_FILE" workflow add --id compute_twap

warpdrive-cli service -f "$SERVICE_FILE" workflow trigger \
    --id compute_twap set-stellar \
    --contract-id "$ORACLE" \
    --chain "$TRIGGER_CHAIN" \
    --topic symbol:twapreq \
    --topic wildcard

warpdrive-cli service -f "$SERVICE_FILE" workflow component \
    --id compute_twap set-source-digest --digest "$TWAP_DIGEST"

warpdrive-cli service -f "$SERVICE_FILE" workflow submit \
    --id compute_twap set-aggregator

warpdrive-cli service -f "$SERVICE_FILE" workflow submit \
    --id compute_twap component set-source-digest --digest "$AGG_DIGEST"

warpdrive-cli service -f "$SERVICE_FILE" workflow submit \
    --id compute_twap component config \
    --values "chain=$TRIGGER_CHAIN" \
    --values "service_handler=$ORACLE"

# ── workflow 3: compute_median (r2ready event → median-circuit → aggregator) ─

warpdrive-cli service -f "$SERVICE_FILE" workflow add --id compute_median

warpdrive-cli service -f "$SERVICE_FILE" workflow trigger \
    --id compute_median set-stellar \
    --contract-id "$ORACLE" \
    --chain "$TRIGGER_CHAIN" \
    --topic symbol:r2ready \
    --topic wildcard

warpdrive-cli service -f "$SERVICE_FILE" workflow component \
    --id compute_median set-source-digest --digest "$MEDIAN_DIGEST"

warpdrive-cli service -f "$SERVICE_FILE" workflow submit \
    --id compute_median set-aggregator

warpdrive-cli service -f "$SERVICE_FILE" workflow submit \
    --id compute_median component set-source-digest --digest "$AGG_DIGEST"

warpdrive-cli service -f "$SERVICE_FILE" workflow submit \
    --id compute_median component config \
    --values "chain=$TRIGGER_CHAIN" \
    --values "service_handler=$ORACLE"

# ── service manager points at on-chain project_root ───────────────────

warpdrive-cli service -f "$SERVICE_FILE" manager set-stellar \
    --chain "$MANAGER_CHAIN" \
    --address "$PROJECT_ROOT"

# ── signature_kind: ed25519 / sep53 (matches the on-chain verifier) ───

tmp=$(mktemp)
jq '. + {signature_kind: {algorithm: "ed25519", prefix: "sep53"}}' \
    "$SERVICE_FILE" > "$tmp" && mv "$tmp" "$SERVICE_FILE"

echo "wrote $SERVICE_FILE"
