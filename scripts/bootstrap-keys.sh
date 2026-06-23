#!/usr/bin/env bash
#
# bootstrap-keys.sh — generate the two stellar identities the oracle-demo
# quickstart needs and print them as a .env-compatible block on stdout.
#
# Usage:
#   ./scripts/bootstrap-keys.sh > .env
#   set -a; source .env; set +a
#
# This wipes any prior `oracle-deployer` / `warpdrive-operator` identities
# in the local `stellar keys` store — they are demo-only and produced fresh
# every time.

set -euo pipefail

stellar keys rm oracle-deployer 2>/dev/null || true
stellar keys rm warpdrive-operator 2>/dev/null || true

stellar keys generate oracle-deployer --fund --network testnet
stellar keys generate warpdrive-operator

DEPLOYER_SECRET=$(stellar keys show oracle-deployer)
DEPLOYER_ADDRESS=$(stellar keys address oracle-deployer)
WARPDRIVE_SIGNING_MNEMONIC="$(stellar keys show warpdrive-operator --phrase)"

cat <<EOF
DEPLOYER_SECRET=$DEPLOYER_SECRET
DEPLOYER_ADDRESS=$DEPLOYER_ADDRESS
WARPDRIVE_SIGNING_MNEMONIC="$WARPDRIVE_SIGNING_MNEMONIC"
EOF
